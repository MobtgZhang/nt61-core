use super::*;

pub(in super::super) const GATEWAY: u32 = 0xfffc;
const WM_COMMAND: u32 = 0x111;
const BFFM_INITIALIZED: u64 = 1;
const BFFM_SELCHANGED: u64 = 2;
const BFFM_ENABLEOK: u32 = 0x465;
const BFFM_SETSELECTIONA: u32 = 0x466;
const BFFM_SETSELECTIONW: u32 = 0x467;
const MAKE_FOLDER: u32 = 3;
const TREE: u32 = 100;
const PATH_EDIT: u32 = 101;
const RETURN_ONLY_FS_DIRS: u32 = 0x1;
const EDIT_BOX: u32 = 0x10;
const NEW_STYLE: u32 = 0x40;
const NO_NEW_FOLDER: u32 = 0x200;
const MAX_NODES: usize = 4096;

struct Node {
    handle: u64,
    parent: u64,
    path: Option<String>,
    loaded: bool,
}

fn child_path(parent: &str, name: &str) -> Option<String> {
    absolute_path(&alloc::format!("{}\\{name}", parent.trim_end_matches('\\')))
}

fn folder_component(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | "..") && !name.contains(['\0', '/', '\\', ':'])
}

fn absolute_path(input: &str) -> Option<String> {
    let input = input.replace('/', "\\");
    let path = input
        .get(..3)?
        .eq_ignore_ascii_case("C:\\")
        .then_some(input.get(3..)?)?;
    let path = path.trim_end_matches('\\');
    if (!path.is_empty() && !path.split('\\').all(folder_component))
        || path.encode_utf16().count() >= shell::MAX_PATH - 3
    {
        return None;
    }
    Some(alloc::format!("C:\\{path}"))
}

fn below_root(root: &str, path: &str) -> bool {
    let root = root.to_lowercase();
    let path = path.to_lowercase();
    path.strip_prefix(&root)
        .is_some_and(|rest| rest.is_empty() || root.ends_with('\\') || rest.starts_with('\\'))
}

pub(super) struct Folder {
    hwnd: u32,
    owner: u32,
    restore_owner: bool,
    display: u64,
    image_index: u64,
    procedure: u64,
    cookie: u64,
    message: u64,
    return_pc: u64,
    abi: GuestAbi,
    root: String,
    root_index: usize,
    namespace: bool,
    desktop_selectable: bool,
    nodes: Vec<Node>,
    selected: usize,
    tree: u32,
    status: u32,
    edit: u32,
    make_folder: u32,
    ok: u32,
    enabled: bool,
    selectable: bool,
    changed: bool,
    accept_selection: bool,
}

fn control(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    parent: u32,
    id: u32,
    text: &str,
    rect: [i16; 4],
) -> Result<u32, ApiError> {
    let (kind, style) = match id {
        0 | w::FOLDER_PICKER_PATH_LABEL => (5, 0x5000_0000),
        PATH_EDIT => (w::EDIT_CONTROL, 0x5001_0000),
        _ => (6, 0x5001_0000),
    };
    dialog::create_window(
        image,
        services,
        window_request(parent, kind, id, style, rect),
        text.into(),
        0,
    )
}

fn window_request(parent: u32, kind: u32, id: u32, style: u32, pixels: [i16; 4]) -> w::Packet {
    let mut request = dialog::window_request(parent, kind, id, style, [0; 4]);
    for (value, pixel) in request.values[13..17].iter_mut().zip(pixels) {
        *value = u32::from(pixel as u16);
    }
    request
}

fn text(services: &mut dyn GuestServices, hwnd: u32, value: &str) -> Result<(), ApiError> {
    let mut request = packet(3, hwnd);
    request.values[8] = 0x000c;
    copied_text(&mut request, value)?;
    send(services, request)?;
    Ok(())
}

fn enable(services: &mut dyn GuestServices, hwnd: u32, enabled: bool) -> Result<(), ApiError> {
    let mut request = packet(w::ENABLE_WINDOW, hwnd);
    request.values[9] = u32::from(enabled);
    send(services, request)?;
    Ok(())
}

fn tree_message(
    services: &mut dyn GuestServices,
    hwnd: u32,
    message: u32,
    wp: u64,
    lp: u64,
    label: &str,
) -> Result<u64, ApiError> {
    let mut request = packet(3, hwnd);
    request.values[8] = message;
    request.values[9] = wp as u32;
    request.values[10] = (wp >> 32) as u32;
    request.values[11] = lp as u32;
    request.values[12] = (lp >> 32) as u32;
    copied_text(&mut request, label)?;
    Ok(scalar(&send(services, request)?))
}

impl Folder {
    fn insert(
        &mut self,
        services: &mut dyn GuestServices,
        parent: u64,
        label: &str,
        path: Option<String>,
        icon: u64,
    ) -> Result<usize, ApiError> {
        let handle = tree_message(services, self.tree, 0x1132, parent, icon, label)?;
        if handle == 0 {
            return Err(8u32.into());
        }
        if path.is_some() {
            tree_message(services, self.tree, w::FOLDER_TREE_SET_EXPANDABLE, 1, handle, "")?;
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            handle,
            parent,
            loaded: path.is_none(),
            path,
        });
        Ok(index)
    }

    fn load(&mut self, services: &mut dyn GuestServices, index: usize) -> Result<bool, ApiError> {
        let node = &self.nodes[index];
        if node.loaded {
            return Ok(true);
        }
        let parent = node.handle;
        let Some(path) = node.path.clone() else { return Ok(true); };
        let Ok(entries) = services.directory_entries(self.abi, &path) else {
            tree_message(services, self.tree, 0x1102, 1, parent, "")?;
            return Ok(false);
        };
        let mut children: Vec<String> = entries
            .into_iter()
            .filter(|entry| entry.attributes & 0x410 == 0x10)
            .filter_map(|entry| String::from_utf16(&entry.name).ok())
            .filter(|name| folder_component(name))
            .collect();
        children.sort_by_cached_key(|name| name.to_lowercase());
        for name in children {
            if self.nodes.len() >= MAX_NODES {
                break;
            }
            if let Some(child) = child_path(&path, &name) {
                self.insert(services, parent, &name, Some(child), w::FOLDER_ICON)?;
            }
        }
        self.nodes[index].loaded = true;
        tree_message(
            services,
            self.tree,
            w::FOLDER_TREE_SET_EXPANDABLE,
            0,
            parent,
            "",
        )?;
        Ok(true)
    }

    fn select(&mut self, services: &mut dyn GuestServices, index: usize) -> Result<(), ApiError> {
        let node = &self.nodes[index];
        let filesystem = node.path.as_ref().is_some_and(|path| services.directory_entries(self.abi, path).is_ok());
        let selectable = filesystem || (index == 0 && self.desktop_selectable);
        if self.selected != index || self.selectable != selectable {
            self.selected = index;
            self.selectable = selectable;
            self.enabled = true;
            self.changed = true;
        }
        tree_message(services, self.tree, 0x110b, 9, node.handle, "")?;
        text(
            services,
            self.status,
            if node.path.is_some() && !filesystem {
                "This folder cannot be opened."
            } else {
                ""
            },
        )?;
        if self.edit != 0 {
            text(services, self.edit, node.path.as_deref().unwrap_or(""))?;
        }
        enable(services, self.ok, self.enabled && self.selectable)?;
        if self.make_folder != 0 {
            enable(services, self.make_folder, filesystem)?;
        }
        Ok(())
    }

    fn make_folder(&mut self, services: &mut dyn GuestServices) -> Result<(), ApiError> {
        if !self.selectable || self.make_folder == 0 {
            return Ok(());
        }
        let selected = self.selected;
        if !self.load(services, selected)? || self.nodes.len() >= MAX_NODES {
            return text(services, self.status, "Unable to create a folder here.");
        }
        let parent = self.nodes[selected].handle;
        let Some(base) = self.nodes[selected].path.clone() else { return Ok(()); };
        for suffix in 1..=100 {
            let name = if suffix == 1 {
                String::from("New folder")
            } else {
                alloc::format!("New folder ({suffix})")
            };
            let Some(path) = child_path(&base, &name) else {
                break;
            };
            match services.create_directory(self.abi, &path) {
                Ok(()) => {
                    let index = self.insert(services, parent, &name, Some(path), w::FOLDER_ICON)?;
                    tree_message(services, self.tree, 0x1102, 2, parent, "")?;
                    return self.select(services, index);
                }
                Err(0xc000_0035 | 0xc000_00ba) => continue,
                Err(_) => break,
            }
        }
        text(services, self.status, "Unable to create a folder here.")
    }

    fn populate(&mut self, services: &mut dyn GuestServices, profile: Option<&str>) -> Result<(), ApiError> {
        let parent = if self.namespace {
            let desktop = self.insert(services, 0, "Desktop", None, w::DESKTOP_ICON)?;
            let desktop = self.nodes[desktop].handle;
            let libraries = self.insert(services, desktop, "Libraries", None, w::LIBRARIES_ICON)?;
            let libraries = self.nodes[libraries].handle;
            if let Some(profile) = profile {
                for name in ["Documents", "Music", "Pictures", "Videos"] {
                    if let Some(path) = child_path(profile, name).filter(|path| {
                        services.directory_entries(self.abi, path).is_ok()
                    }) {
                        self.insert(services, libraries, name, Some(path), w::FOLDER_ICON)?;
                    }
                }
                let name = profile.rsplit('\\').next().ok_or(INVALID_PARAMETER)?;
                self.insert(services, desktop, name, Some(profile.into()), w::PROFILE_ICON)?;
            }
            let computer = self.insert(services, desktop, "Computer", None, w::COMPUTER_ICON)?;
            self.nodes[computer].handle
        } else {
            0
        };
        let drive = self.root == "C:\\";
        let root = self.root.clone();
        let label = if drive { "Local Disk (C:)" } else { root.rsplit('\\').next().ok_or(INVALID_PARAMETER)? };
        self.root_index = self.insert(services, parent, label, Some(root.clone()), if drive { w::DRIVE_ICON } else { w::FOLDER_ICON })?;
        if self.namespace {
            tree_message(services, self.tree, 0x1102, 2, self.nodes[0].handle, "")?;
            tree_message(services, self.tree, 0x1102, 2, parent, "")?;
        }
        self.load(services, self.root_index)?;
        tree_message(services, self.tree, 0x1102, 2, self.nodes[self.root_index].handle, "")?;
        self.select(services, if self.namespace { 0 } else { self.root_index })
    }

    fn apply_edit(&mut self, services: &mut dyn GuestServices) -> Result<bool, ApiError> {
        if self.edit == 0 {
            return Ok(true);
        }
        let mut request = packet(3, self.edit);
        request.values[8] = 0x000d;
        let reply = send(services, request)?;
        let value = reply.string(0).ok_or(INVALID_PARAMETER)?;
        let current = self.nodes[self.selected].path.as_deref().unwrap_or("");
        if value.eq_ignore_ascii_case(current) {
            return Ok(true);
        }
        if navigate(self, services, value)? {
            return Ok(true);
        }
        text(services, self.status, "Enter the full path to an accessible folder.")?;
        Ok(false)
    }
}

fn put_text(image: &mut GuestImage, pointer: u64, value: &str) -> Result<(), ApiError> {
    let bytes: Vec<u8> = value
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    if !image.memory.write_virt(pointer, &bytes) {
        return Err(ApiError::MemoryFault);
    }
    Ok(())
}

fn procedure(image: &mut GuestImage, message: u64, pidl: u64) -> Result<Outcome, ApiError> {
    let folder = image
        .win32
        .local
        .user
        .folders
        .last()
        .ok_or(INVALID_PARAMETER)?;
    let hwnd = folder.hwnd;
    let procedure = folder.procedure;
    let cookie = folder.cookie;
    image.win32.local.user.completion = Some(Completion::BrowseFolder { hwnd, pidl });
    enter_callback(image, procedure, [u64::from(hwnd), message, pidl, cookie])?;
    Ok(Outcome::Continue)
}

fn resume(image: &mut GuestImage) -> Result<Outcome, ApiError> {
    image.thread.context.rip = image
        .win32
        .user
        .browse_trampoline
        .ok_or(INVALID_PARAMETER)?;
    Ok(Outcome::Continue)
}

fn cleanup(image: &mut GuestImage, services: &mut dyn GuestServices, folder: &Folder) {
    if dialog::remove(image, services, folder.hwnd).is_err() {
        image.win32.user.remove_windows(folder.hwnd);
    }
    if folder.restore_owner {
        let _ = enable(services, folder.owner, true);
    }
    let _ = heap_free(image, folder.message);
    image.thread.context.rip = folder.return_pc;
}

fn open(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    address: u64,
) -> Result<Outcome, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let abi = if x86 {
        GuestAbi::X86_32
    } else {
        GuestAbi::X86_64
    };
    let image_offset = if x86 { 28 } else { 56 };
    read(&mut image.memory, address, image_offset + 4)?;
    let owner = u32::try_from(word(&mut image.memory, address, width)?).map_err(|_| 1400u32)?;
    let root = word(&mut image.memory, address + width as u64, width)?;
    let display = word(&mut image.memory, address + 2 * width as u64, width)?;
    let title = word(&mut image.memory, address + 3 * width as u64, width)?;
    let flags = word(&mut image.memory, address + 4 * width as u64, 4)? as u32;
    let callback = word(&mut image.memory, address + 5 * width as u64, width)?;
    let cookie = word(&mut image.memory, address + 6 * width as u64, width)?;
    let image_index = address + image_offset as u64;
    if flags & !(RETURN_ONLY_FS_DIRS | EDIT_BOX | NEW_STYLE | NO_NEW_FOLDER) != 0
        || image.win32.local.user.folders.len() >= 8
    {
        return Err(120u32.into());
    }
    if !image.memory.probe_write(display, shell::MAX_PATH * 2)
        || !image.memory.probe_write(image_index, 4)
    {
        return Err(ApiError::MemoryFault);
    }
    put_text(image, display, "")?;
    if owner != 0
        && !image
            .win32
            .user
            .windows
            .iter()
            .any(|window| window.hwnd == owner)
    {
        return Err(1400u32.into());
    }
    let namespace = root == 0 || word(&mut image.memory, root, 2)? == 0;
    let root = if namespace {
        String::from("C:\\")
    } else {
        let path = shell::pidl_path(image, root)?.ok_or(120u32)?;
        absolute_path(&String::from_utf16(&path).map_err(|_| INVALID_PARAMETER)?)
            .ok_or(INVALID_PARAMETER)?
    };
    services
        .directory_entries(abi, &root)
        .map_err(status_error)?;
    let profile = if namespace {
        let variable: Vec<u16> = "USERPROFILE".encode_utf16().collect();
        super::super::environment::lookup(image, &variable)?
            .and_then(|units| String::from_utf16(&units).ok())
            .and_then(|path| absolute_path(&path))
            .filter(|path| path != "C:\\" && services.directory_entries(abi, path).is_ok())
    } else {
        None
    };
    let title = if title == 0 {
        String::new()
    } else {
        string(&mut image.memory, title, true)?
    };
    if image.win32.user.browse_trampoline.is_none() {
        image.win32.user.browse_trampoline = Some(dialog::create_trampoline(image, GATEWAY)?);
    }
    let message = heap_allocate(image, 48)?;
    let modern = flags & NEW_STYLE != 0;
    let edit_box = flags & EDIT_BOX != 0;
    let (width, height) = if modern { (535, 496) } else { (368, 343) };
    let style = if modern { 0x0004_0000 } else { 0 };
    let mut request = window_request(0, 0, 0, style, [0, 0, width, height]);
    request.values[23] = w::FOLDER_PICKER_WINDOW;
    let hwnd = match dialog::create_window(image, services, request, "Browse For Folder".into(), 0)
    {
        Ok(hwnd) => hwnd,
        Err(error) => {
            let _ = heap_free(image, message);
            return Err(error);
        }
    };
    let mut folder = Folder {
        hwnd,
        owner,
        restore_owner: false,
        display,
        image_index,
        procedure: callback,
        cookie,
        message,
        return_pc: image.thread.context.rip,
        abi,
        root,
        root_index: 0,
        namespace,
        desktop_selectable: namespace && flags & RETURN_ONLY_FS_DIRS == 0,
        nodes: Vec::new(),
        selected: 0,
        tree: 0,
        status: 0,
        edit: 0,
        make_folder: 0,
        ok: 0,
        enabled: true,
        selectable: true,
        changed: !namespace,
        accept_selection: false,
    };
    let result = (|| {
        let inner_width = width - 12;
        let buttons_y = height - 71;
        let tree_top = if modern { 55 } else { 44 };
        let tree_bottom = buttons_y - if edit_box { 47 } else { 15 };
        control(image, services, hwnd, 0, &title, [16, 10, inner_width - 32, 23])?;
        folder.status = control(image, services, hwnd, 0, "", [16, tree_top - 22, inner_width - 32, 20])?;
        folder.tree = dialog::create_window(
            image,
            services,
            window_request(
                hwnd,
                w::FOLDER_TREE_CONTROL,
                TREE,
                0x5001_0000,
                [16, tree_top, inner_width - 32, tree_bottom - tree_top],
            ),
            String::new(),
            0,
        )?;
        if edit_box {
            control(image, services, hwnd, w::FOLDER_PICKER_PATH_LABEL, "Folder:", [16, buttons_y - 31, 53, 23])?;
            folder.edit = control(image, services, hwnd, PATH_EDIT, "", [69, buttons_y - 35, inner_width - 85, 24])?;
        }
        if modern && flags & NO_NEW_FOLDER == 0 {
            folder.make_folder = control(
                image,
                services,
                hwnd,
                MAKE_FOLDER,
                "Make New Folder",
                [16, buttons_y, 129, 24],
            )?;
        }
        folder.ok = control(image, services, hwnd, w::FOLDER_PICKER_OK, "OK", [inner_width - 186, buttons_y, 81, 24])?;
        control(image, services, hwnd, w::FOLDER_PICKER_CANCEL, "Cancel", [inner_width - 97, buttons_y, 81, 24])?;
        folder.populate(services, profile.as_deref())?;
        folder.changed = !namespace;
        if owner != 0 {
            folder.restore_owner =
                invoke(image, "EnableWindow", &[u64::from(owner), 0], services)? == 0;
        }
        invoke(image, "ShowWindow", &[u64::from(hwnd), 5], services)?;
        invoke(image, "SetActiveWindow", &[u64::from(hwnd)], services)?;
        invoke(image, "SetFocus", &[u64::from(folder.tree)], services)?;
        Ok::<_, ApiError>(())
    })();
    if let Err(error) = result {
        cleanup(image, services, &folder);
        return Err(error);
    }
    image.win32.local.user.folders.push(folder);
    if callback == 0 {
        resume(image)
    } else {
        procedure(image, BFFM_INITIALIZED, 0)
    }
}

pub(in super::super) fn start(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    address: u64,
) -> Outcome {
    let depth = image.win32.local.user.folders.len();
    match open(image, services, address) {
        Ok(outcome) => outcome,
        Err(failure) => {
            if image.win32.local.user.folders.len() > depth {
                let _ = finish(image, services, false);
            }
            report_failure(image, failure)
        }
    }
}

fn report_failure(image: &mut GuestImage, failure: ApiError) -> Outcome {
    let code = match failure {
        ApiError::Windows(code) => code,
        ApiError::MemoryFault => NOACCESS,
        _ => 120,
    };
    error(image, code);
    Outcome::Return(0)
}

fn selection(
    image: &mut GuestImage,
    message: u32,
    wp: u64,
    lp: u64,
) -> Result<Option<String>, ApiError> {
    if wp == 0 {
        return Ok(shell::pidl_path(image, lp)?.and_then(|units| String::from_utf16(&units).ok()));
    }
    Ok(Some(string(
        &mut image.memory,
        lp,
        message == BFFM_SETSELECTIONW,
    )?))
}

fn navigate(
    folder: &mut Folder,
    services: &mut dyn GuestServices,
    target: &str,
) -> Result<bool, ApiError> {
    let Some(path) = absolute_path(target).filter(|path| below_root(&folder.root, path)) else {
        return Ok(false);
    };
    let root_depth = folder.root.trim_end_matches('\\').split('\\').count();
    let mut index = folder.root_index;
    let mut ancestors = Vec::new();
    for component in path.trim_end_matches('\\').split('\\').skip(root_depth) {
        if !folder.load(services, index)? {
            return Ok(false);
        }
        let parent = folder.nodes[index].handle;
        let component = component.to_lowercase();
        let Some(child) = folder.nodes.iter().position(|node| {
            node.parent == parent
                && node
                    .path.as_deref()
                    .and_then(|path| path.rsplit('\\').next())
                    .is_some_and(|name| name.to_lowercase() == component)
        }) else {
            return Ok(false);
        };
        ancestors.push(parent);
        index = child;
    }
    if !ancestors.is_empty() && services.directory_entries(folder.abi, &path).is_err() {
        return Ok(false);
    }
    for ancestor in ancestors {
        tree_message(services, folder.tree, 0x1102, 2, ancestor, "")?;
    }
    folder.select(services, index)?;
    Ok(folder.selectable)
}

pub(in super::super) fn send_message(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    args: &[u64],
) -> Option<Result<u64, ApiError>> {
    let folder = image.win32.local.user.folders.last()?;
    if u64::from(folder.hwnd) != args[0] {
        return None;
    }
    let message = args[1] as u32;
    Some(match message {
        BFFM_ENABLEOK | BFFM_SETSELECTIONA | BFFM_SETSELECTIONW => handle_message(
            image,
            services,
            Message {
                hwnd: args[0],
                message,
                wp: args[2],
                lp: args[3],
            },
        )
        .map(|_| 0),
        WM_COMMAND | 0x10 => queue(
            image,
            Message {
                hwnd: args[0],
                message,
                wp: args[2],
                lp: args[3],
            },
        )
        .map(|_| 0),
        _ => return None,
    })
}

fn handle_message(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    message: Message,
) -> Result<Outcome, ApiError> {
    let path = if matches!(message.message, BFFM_SETSELECTIONA | BFFM_SETSELECTIONW) {
        selection(image, message.message, message.wp, message.lp)?
    } else {
        None
    };
    let folder = image
        .win32
        .local
        .user
        .folders
        .last_mut()
        .ok_or(INVALID_PARAMETER)?;
    match message.message {
        BFFM_ENABLEOK => {
            folder.enabled = message.lp != 0;
            enable(services, folder.ok, folder.enabled && folder.selectable)?;
        }
        w::FOLDER_PATH_CHANGED if message.lp == u64::from(folder.edit) => {
            enable(services, folder.ok, folder.enabled && (message.wp != 0 || folder.selectable))?;
        }
        w::FOLDER_TREE_SELECTED | w::FOLDER_TREE_EXPANDED | w::FOLDER_TREE_ACTIVATED
            if message.lp == u64::from(folder.tree) =>
        {
            if let Some(index) = folder
                .nodes
                .iter()
                .position(|node| node.handle == message.wp)
            {
                match message.message {
                    w::FOLDER_TREE_SELECTED => folder.select(services, index)?,
                    w::FOLDER_TREE_EXPANDED => {
                        folder.load(services, index)?;
                    }
                    _ if index == folder.selected && folder.enabled && folder.selectable => {
                        return finish(image, services, true);
                    }
                    _ => {}
                }
            }
        }
        BFFM_SETSELECTIONA | BFFM_SETSELECTIONW => {
            if let Some(path) = path {
                navigate(folder, services, &path)?;
            }
        }
        0x10 | 0x12 => return finish(image, services, false),
        0x102 if message.wp == 27 => return finish(image, services, false),
        WM_COMMAND if message.wp >> 16 == 0 => match message.wp as u32 {
            w::FOLDER_PICKER_OK => {
                if folder.apply_edit(services)? && folder.enabled && folder.selectable {
                    if folder.changed && folder.procedure != 0 {
                        folder.accept_selection = true;
                    } else {
                        return finish(image, services, true);
                    }
                }
            }
            w::FOLDER_PICKER_CANCEL => return finish(image, services, false),
            MAKE_FOLDER => folder.make_folder(services)?,
            _ => {}
        },
        _ => {}
    }
    Ok(Outcome::Retry)
}

fn finish(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    accept: bool,
) -> Result<Outcome, ApiError> {
    let folder = image
        .win32
        .local
        .user
        .folders
        .pop()
        .ok_or(INVALID_PARAMETER)?;
    cleanup(image, services, &folder);
    if !accept {
        return Ok(Outcome::Return(0));
    }
    let (name, bytes) = if let Some(path) = folder.nodes[folder.selected].path.as_deref() {
        services.directory_entries(folder.abi, path).map_err(status_error)?;
        let name = if path.ends_with('\\') { path } else { path.rsplit('\\').next().ok_or(INVALID_PARAMETER)? };
        (name, shell::path_pidl(&path.encode_utf16().collect::<Vec<_>>()))
    } else if folder.selected == 0 && folder.desktop_selectable {
        ("Desktop", vec![0, 0])
    } else {
        return Err(INVALID_PARAMETER.into());
    };
    let pidl = shell::task_alloc(image, bytes.len() as u64)?;
    let result = (|| {
        if !image.memory.write_virt(pidl, &bytes) {
            return Err(ApiError::MemoryFault);
        }
        put_text(image, folder.display, name)?;
        put(&mut image.memory, folder.image_index, 0, 4)?;
        Ok(Outcome::Return(pidl))
    })();
    if result.is_err() {
        let _ = shell::task_free(image, pidl);
    }
    result
}

fn poll_inner(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
) -> Result<Outcome, ApiError> {
    let folder = image
        .win32
        .local
        .user
        .folders
        .last_mut()
        .ok_or(INVALID_PARAMETER)?;
    let address = folder.message;
    let hwnd = u64::from(folder.hwnd);
    if core::mem::take(&mut folder.changed) && folder.procedure != 0 {
        let bytes = match folder.nodes[folder.selected].path.as_deref() {
            Some(path) => shell::path_pidl(&path.encode_utf16().collect::<Vec<_>>()),
            None if folder.selected == 0 => vec![0, 0],
            None => vec![4, 0, 0x4e, 0x54, 0, 0],
        };
        let pidl = heap_allocate(image, bytes.len() as u64)?;
        if !image.memory.write_virt(pidl, &bytes) {
            let _ = heap_free(image, pidl);
            return Err(ApiError::MemoryFault);
        }
        let result = procedure(image, BFFM_SELCHANGED, pidl);
        if result.is_err() {
            let _ = heap_free(image, pidl);
        }
        return result;
    }
    if core::mem::take(&mut folder.accept_selection) && folder.enabled && folder.selectable {
        return finish(image, services, true);
    }
    match receive_message(image, services, &[address, hwnd, 0, 0], false) {
        Err(ApiError::Retry) => return Ok(Outcome::Retry),
        result => {
            result?;
        }
    }
    let message = message_read(image, address)?;
    if message.message == 0x12 {
        queue(image, message)?;
    }
    handle_message(image, services, message)
}

pub(in super::super) fn poll(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
) -> Result<Outcome, StopReason> {
    let depth = image.win32.local.user.folders.len();
    match poll_inner(image, services) {
        Ok(outcome) => Ok(outcome),
        Err(failure) if depth != 0 => {
            if image.win32.local.user.folders.len() == depth {
                let _ = finish(image, services, false);
            }
            Ok(report_failure(image, failure))
        }
        Err(_) => Err(StopReason::DataFault {
            pc: image.thread.context.rip,
        }),
    }
}

pub(super) fn callback_complete(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    hwnd: u32,
    pidl: u64,
) -> Result<Outcome, ApiError> {
    if pidl != 0 {
        heap_free(image, pidl)?;
    }
    if !image
        .win32
        .user
        .windows
        .iter()
        .any(|window| window.hwnd == hwnd)
    {
        return finish(image, services, false);
    }
    resume(image)
}
