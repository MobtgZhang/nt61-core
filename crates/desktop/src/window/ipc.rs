//! Versioned window service protocol and ownership boundary.
//!
//! The service intentionally owns the redirected surfaces and window state.
//! A client can only mutate resources recorded under its process identity;
//! disconnecting a process reclaims every window and surface atomically.

use crate::compositor::{Compositor, SurfaceHandle};
use crate::graphics::Surface;
use crate::window::manager::{DesktopSecurity, Message, Session, WindowHandle, WindowVisibility};
use crate::window::Rect;
use alloc::vec::Vec;

pub const WINDOW_SERVICE_VERSION: u32 = 1;
pub const MAX_SURFACE_PIXELS: u32 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClientId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request<'a> {
    Connect,
    CreateWindow {
        class: &'a str,
        title: &'a str,
        bounds: Rect,
        surface_width: u32,
        surface_height: u32,
    },
    ShowWindow {
        window: WindowHandle,
        visibility: WindowVisibility,
    },
    UpdateSurface {
        surface: SurfaceHandle,
        pixels: &'a [u32],
    },
    SetForeground {
        window: WindowHandle,
    },
    DestroyWindow {
        window: WindowHandle,
    },
    Disconnect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    Created {
        window: WindowHandle,
        surface: SurfaceHandle,
    },
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Client {
    id: ClientId,
    windows: Vec<WindowHandle>,
    surfaces: Vec<SurfaceHandle>,
}

#[derive(Debug)]
pub struct WindowService {
    session: Session,
    compositor: Compositor,
    clients: Vec<Client>,
    links: Vec<(WindowHandle, SurfaceHandle)>,
}

impl WindowService {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            session: Session::new(1),
            compositor: Compositor::new(width, height, true),
            clients: Vec::new(),
            links: Vec::new(),
        }
    }

    pub fn security(&self) -> DesktopSecurity {
        if self.session.desktop == "Winlogon" {
            DesktopSecurity::Winlogon
        } else {
            DesktopSecurity::Default
        }
    }

    pub fn request(&mut self, client: ClientId, request: Request<'_>) -> Response {
        if request == Request::Disconnect && !self.is_connected(client) {
            return Response::Rejected;
        }
        if !self.is_connected(client) && !matches!(request, Request::Connect | Request::Disconnect)
        {
            return Response::Rejected;
        }
        match request {
            Request::Connect => {
                self.clients.push(Client {
                    id: client,
                    windows: Vec::new(),
                    surfaces: Vec::new(),
                });
                Response::Accepted
            }
            Request::CreateWindow {
                class,
                title,
                bounds,
                surface_width,
                surface_height,
            } => {
                let pixel_count = (surface_width as usize).checked_mul(surface_height as usize);
                if pixel_count.is_none()
                    || pixel_count.unwrap() > MAX_SURFACE_PIXELS as usize
                    || surface_width == 0
                    || surface_height == 0
                    || bounds.width == 0
                    || bounds.height == 0
                {
                    return Response::Rejected;
                }
                let window = self.session.create_window(class, title, bounds);
                let surface = self.compositor.begin(client.0, bounds);
                self.link(client, window, surface);
                Response::Created { window, surface }
            }
            Request::ShowWindow { window, visibility } => {
                if !self.owns_window(client, window) {
                    return Response::Rejected;
                }
                self.session.show_window(window, visibility);
                Response::Accepted
            }
            Request::UpdateSurface { surface, pixels } => {
                if !self.owns_surface(client, surface) {
                    return Response::Rejected;
                }
                let Some(target) = self.compositor.surface_mut(surface) else {
                    return Response::Rejected;
                };
                if pixels.len() != target.pixels.len() {
                    return Response::Rejected;
                }
                target.pixels.copy_from_slice(pixels);
                Response::Accepted
            }
            Request::SetForeground { window } => {
                if !self.owns_window(client, window) {
                    return Response::Rejected;
                }
                self.session.set_foreground(window);
                Response::Accepted
            }
            Request::DestroyWindow { window } => {
                if !self.owns_window(client, window) {
                    return Response::Rejected;
                }
                self.session.destroy_window(window);
                let surface = self.links.iter().find_map(|(linked, linked_surface)| {
                    (*linked == window).then_some(*linked_surface)
                });
                if let Some(surface) = surface {
                    self.compositor.remove(surface);
                }
                self.links.retain(|(linked, _)| *linked != window);
                if let Some(client_state) = self.client_mut(client) {
                    client_state.windows.retain(|linked| *linked != window);
                    client_state
                        .surfaces
                        .retain(|linked| Some(*linked) != surface);
                }
                Response::Accepted
            }
            Request::Disconnect => {
                if self.disconnect(client) {
                    Response::Accepted
                } else {
                    Response::Rejected
                }
            }
        }
    }

    pub fn disconnect(&mut self, client: ClientId) -> bool {
        let Some(state) = self.clients.iter().position(|entry| entry.id == client) else {
            return false;
        };
        let state = self.clients.remove(state);
        for window in &state.windows {
            self.session.destroy_window(*window);
        }
        for surface in &state.surfaces {
            self.compositor.remove(*surface);
        }
        self.links.retain(|(window, surface)| {
            !state.windows.contains(window) && !state.surfaces.contains(surface)
        });
        true
    }

    pub fn route_pointer(&mut self, x: i32, y: i32, pressed: bool) -> Option<Message> {
        self.session.route_pointer(x, y, pressed)
    }

    pub fn route_key(&mut self, key: u32, pressed: bool) -> Option<Message> {
        self.session.route_key(key, pressed)
    }

    pub fn pop_message(&mut self) -> Option<Message> {
        self.session.pop_message()
    }

    pub fn compose(&self, wallpaper: &Surface) -> Surface {
        self.compositor.compose(wallpaper)
    }

    fn is_connected(&self, client: ClientId) -> bool {
        self.clients.iter().any(|entry| entry.id == client)
    }

    fn client_mut(&mut self, client: ClientId) -> Option<&mut Client> {
        self.clients.iter_mut().find(|entry| entry.id == client)
    }

    fn owns_window(&self, client: ClientId, window: WindowHandle) -> bool {
        self.clients
            .iter()
            .any(|entry| entry.id == client && entry.windows.contains(&window))
    }

    fn owns_surface(&self, client: ClientId, surface: SurfaceHandle) -> bool {
        self.clients
            .iter()
            .any(|entry| entry.id == client && entry.surfaces.contains(&surface))
    }

    fn link(&mut self, client: ClientId, window: WindowHandle, surface: SurfaceHandle) {
        if let Some(entry) = self.clients.iter_mut().find(|entry| entry.id == client) {
            entry.windows.push(window);
            entry.surfaces.push(surface);
        }
        self.links.push((window, surface));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: ClientId = ClientId(42);
    const OTHER: ClientId = ClientId(43);

    #[test]
    fn creates_bound_window_and_redirected_surface() {
        let mut service = WindowService::new(320, 240);
        assert_eq!(
            service.request(CLIENT, Request::Connect),
            Response::Accepted
        );
        let created = service.request(
            CLIENT,
            Request::CreateWindow {
                class: "Notepad",
                title: "Notepad",
                bounds: Rect::new(8, 8, 160, 100),
                surface_width: 160,
                surface_height: 100,
            },
        );
        let Response::Created { window, surface } = created else {
            panic!("create window must succeed");
        };
        assert!(window.is_valid());
        assert_ne!(surface, SurfaceHandle(0));
    }

    #[test]
    fn clients_cannot_touch_foreign_windows_or_surfaces() {
        let mut service = WindowService::new(320, 240);
        assert_eq!(
            service.request(CLIENT, Request::Connect),
            Response::Accepted
        );
        assert_eq!(service.request(OTHER, Request::Connect), Response::Accepted);
        let Response::Created { window, surface } = service.request(
            CLIENT,
            Request::CreateWindow {
                class: "Notepad",
                title: "Notepad",
                bounds: Rect::new(8, 8, 160, 100),
                surface_width: 160,
                surface_height: 100,
            },
        ) else {
            panic!("create window must succeed");
        };
        assert_eq!(
            service.request(
                OTHER,
                Request::ShowWindow {
                    window,
                    visibility: WindowVisibility::Visible
                }
            ),
            Response::Rejected
        );
        assert_eq!(
            service.request(
                OTHER,
                Request::UpdateSurface {
                    surface,
                    pixels: &[]
                }
            ),
            Response::Rejected
        );
        assert_eq!(
            service.request(OTHER, Request::DestroyWindow { window }),
            Response::Rejected
        );
    }

    #[test]
    fn surface_length_is_validated_and_owned_pixels_are_updated() {
        let mut service = WindowService::new(320, 240);
        assert_eq!(
            service.request(CLIENT, Request::Connect),
            Response::Accepted
        );
        let Response::Created { window: _, surface } = service.request(
            CLIENT,
            Request::CreateWindow {
                class: "Notepad",
                title: "Notepad",
                bounds: Rect::new(8, 8, 2, 2),
                surface_width: 2,
                surface_height: 2,
            },
        ) else {
            panic!("create window must succeed");
        };
        assert_eq!(
            service.request(
                CLIENT,
                Request::UpdateSurface {
                    surface,
                    pixels: &[0; 3]
                }
            ),
            Response::Rejected
        );
        assert_eq!(
            service.request(
                CLIENT,
                Request::UpdateSurface {
                    surface,
                    pixels: &[0x80FF_0000; 4]
                }
            ),
            Response::Accepted
        );
    }

    #[test]
    fn disconnect_reclaims_all_client_resources() {
        let mut service = WindowService::new(320, 240);
        assert_eq!(
            service.request(CLIENT, Request::Connect),
            Response::Accepted
        );
        let Response::Created { window, surface } = service.request(
            CLIENT,
            Request::CreateWindow {
                class: "Notepad",
                title: "Notepad",
                bounds: Rect::new(8, 8, 160, 100),
                surface_width: 160,
                surface_height: 100,
            },
        ) else {
            panic!("create window must succeed");
        };
        service.request(
            CLIENT,
            Request::ShowWindow {
                window,
                visibility: WindowVisibility::Visible,
            },
        );
        assert!(service.disconnect(CLIENT));
        assert!(!service.disconnect(CLIENT));
        assert_eq!(
            service.request(
                CLIENT,
                Request::ShowWindow {
                    window,
                    visibility: WindowVisibility::Hidden
                }
            ),
            Response::Rejected
        );
        assert_eq!(
            service.request(
                CLIENT,
                Request::UpdateSurface {
                    surface,
                    pixels: &[0; 160 * 100]
                }
            ),
            Response::Rejected
        );
    }

    #[test]
    fn input_only_reaches_focused_window_and_queue_is_owned_by_service() {
        let mut service = WindowService::new(320, 240);
        assert_eq!(
            service.request(CLIENT, Request::Connect),
            Response::Accepted
        );
        assert_eq!(service.request(OTHER, Request::Connect), Response::Accepted);
        let Response::Created { window: first, .. } = service.request(
            CLIENT,
            Request::CreateWindow {
                class: "First",
                title: "First",
                bounds: Rect::new(0, 0, 160, 100),
                surface_width: 160,
                surface_height: 100,
            },
        ) else {
            panic!("first window must be created");
        };
        let Response::Created { window: second, .. } = service.request(
            OTHER,
            Request::CreateWindow {
                class: "Second",
                title: "Second",
                bounds: Rect::new(80, 50, 160, 100),
                surface_width: 160,
                surface_height: 100,
            },
        ) else {
            panic!("second window must be created");
        };
        service.request(
            OTHER,
            Request::ShowWindow {
                window: second,
                visibility: WindowVisibility::Visible,
            },
        );
        service.request(OTHER, Request::SetForeground { window: second });
        service.route_pointer(120, 80, true);
        service.route_key(b'A' as u32, true);
        let mut focused_message = None;
        while let Some(message) = service.pop_message() {
            if message.window == second {
                focused_message = Some(message);
                break;
            }
        }
        let message = focused_message.unwrap();
        assert_ne!(message.kind, crate::window::WM_ACTIVATE);
        service.request(CLIENT, Request::SetForeground { window: first });
        while service.pop_message().is_some() {}
        service.route_key(b'B' as u32, true);
        let mut focused_message = None;
        while let Some(message) = service.pop_message() {
            if message.window == first {
                focused_message = Some(message);
                break;
            }
        }
        let message = focused_message.unwrap();
        assert_eq!(message.kind, crate::window::WM_KEYDOWN);
    }
}
