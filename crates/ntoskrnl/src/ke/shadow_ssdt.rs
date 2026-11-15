//! Shadow SSDT (Win32k) Implementation
//
//! The Shadow SSDT contains system services provided by win32k.sys,
//! the kernel-mode part of the Windows subsystem. These services
//! handle graphics (GDI) and window management (USER).
//
//! ## Syscall Numbers
//
//! All syscall numbers are based on j00ru/windows-syscalls project
//! for Windows 7 SP1 x64.
//
//! ## References
//! - j00ru/windows-syscalls: https://github.com/j00ru/windows-syscalls

/// Maximum number of shadow services (0x1000 entries for 0x1000-0x1FFF range)
pub const SHADOW_MAX_SERVICES: usize = 0x1000;

/// Shadow service table entry
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ShadowServiceEntry {
    pub handler: *const (),
}

impl ShadowServiceEntry {
    pub const fn new() -> Self {
        Self {
            handler: core::ptr::null(),
        }
    }

    pub const fn is_null(&self) -> bool {
        self.handler.is_null()
    }
}

/// Shadow SSDT - Win32k service table
static mut SHADOW_SERVICE_TABLE: [ShadowServiceEntry; SHADOW_MAX_SERVICES] =
    [ShadowServiceEntry::new(); SHADOW_MAX_SERVICES];

/// Shadow argument count table
static mut SHADOW_ARGUMENT_TABLE: [u8; SHADOW_MAX_SERVICES] = [0; SHADOW_MAX_SERVICES];

// =====================================================================
// Shadow SSDT Initialization
// =====================================================================

/// Initialize the Shadow SSDT
pub fn init() {
    crate::hal::serial::write_string("[ke.shadow_ssdt] enter\r\n");
    // // kprintln!("[SHADOW SSDT] Initializing Shadow System Service Dispatch Table...");
    // // kprintln!("[SHADOW SSDT] Shadow SSDT initialized with {} max services", SHADOW_MAX_SERVICES);
}

/// Initialize and register all win32k shadow services
///
/// This function should be called during kernel initialization to register
/// all GDI and USER service handlers with the Shadow SSDT.
///
/// # Safety
/// This function modifies static mutable arrays. It should only be called
/// during initialization when no other cores are running.
pub unsafe fn init_shadow_services() {
    use crate::libs::win32k::syscall;

    // SAFETY: the caller serializes initialization before other CPUs run.
    for index in 0..SHADOW_MAX_SERVICES {
        SHADOW_SERVICE_TABLE[index] = ShadowServiceEntry::new();
        SHADOW_ARGUMENT_TABLE[index] = 0;
    }

    // Resolve each service by its canonical Windows 7 SP1 name. Numeric
    // aliases previously sent unrelated services to the same handler.
    // Argument sizes are 64-bit slots on every supported 64-bit ISA.
    // Counts above four were checked against W32pArgumentTable in the
    // supplied SP1 win32k.sys (encoded as four bytes per stack argument).
    // Smaller counts use ReactOS w32ksvc64.h signatures; its syscall order
    // is Windows 2003 and must never be used for Windows 7 numbering.
    macro_rules! register {
        ($name:literal, $handler:ident, $arguments:literal) => {
            if let Some(number) = nt61_abi::syscalls::win32k_number($name) {
                let index = usize::from(number - 0x1000);
                assert_eq!(
                    ($arguments as u8).saturating_sub(4),
                    nt61_abi::syscall_metadata::WIN32K_STACK_ARGUMENTS[index],
                    "win32k argument count differs from the original W32pArgumentTable"
                );
                register_shadow_service(
                    index as u32,
                    syscall::$handler as *const (),
                    $arguments * 8,
                );
            }
        };
    }

    register!("NtGdiIntersectClipRect", nt_gdi_intersect_clip_rect, 5);
    register!("NtGdiExtSelectClipRgn", nt_gdi_ext_select_clip_rgn, 3);
    register!("NtGdiOffsetClipRgn", nt_gdi_offset_clip_rgn, 3);
    register!("NtGdiSetMetaRgn", nt_gdi_set_meta_rgn, 1);
    register!("NtGdiGetRandomRgn", nt_gdi_get_random_rgn, 3);
    register!("NtGdiPtVisible", nt_gdi_pt_visible, 3);
    register!("NtGdiRectVisible", nt_gdi_rect_visible, 2);
    register!("NtGdiEqualRgn", nt_gdi_equal_rgn, 2);
    register!("NtGdiPtInRegion", nt_gdi_pt_in_region, 3);
    register!("NtGdiSetRectRgn", nt_gdi_set_rect_rgn, 5);
    register!("NtGdiGetRegionData", nt_gdi_get_region_data, 3);

    register!("NtUserPeekMessage", nt_user_peek_message, 5);
    register!("NtUserCallOneParam", nt_user_call_one_param, 2);
    register!("NtUserGetKeyState", nt_user_get_key_state, 1);
    register!("NtUserInvalidateRect", nt_user_invalidate_rect, 3);
    register!("NtUserCallNoParam", nt_user_call_no_param, 1);
    register!("NtUserGetMessage", nt_user_get_message, 4);
    register!("NtUserMessageCall", nt_user_message_call, 7);
    register!("NtGdiBitBlt", nt_gdi_bit_blt, 11);
    register!("NtGdiGetCharSet", nt_gdi_get_char_set, 1);
    register!("NtUserGetDC", nt_user_get_dc, 1);
    register!("NtGdiSelectBitmap", nt_gdi_select_object, 2);
    register!("NtUserWaitMessage", nt_user_wait_message, 0);
    register!("NtUserTranslateMessage", nt_user_translate_message, 2);
    register!("NtUserGetProp", nt_user_get_prop, 2);
    register!("NtUserPostMessage", nt_user_post_message, 4);
    register!("NtUserQueryWindow", nt_user_query_window, 2);
    register!(
        "NtUserTranslateAccelerator",
        nt_user_translate_accelerator,
        3
    );
    register!("NtUserRedrawWindow", nt_user_redraw_window, 4);
    register!("NtUserWindowFromPoint", nt_user_window_from_point, 2);
    register!("NtUserCallMsgFilter", nt_user_call_msg_filter, 2);
    register!(
        "NtUserValidateTimerCallback",
        nt_user_validate_timer_callback,
        1
    );
    register!("NtUserBeginPaint", nt_user_begin_paint, 2);
    register!("NtUserSetTimer", nt_user_set_timer, 4);
    register!("NtUserEndPaint", nt_user_end_paint, 2);
    register!("NtUserSetCursor", nt_user_set_cursor, 1);
    register!("NtUserKillTimer", nt_user_kill_timer, 2);
    register!("NtUserBuildHwndList", nt_user_build_hwnd_list, 7);
    register!("NtUserSelectPalette", nt_user_select_palette, 3);
    register!("NtUserCallNextHookEx", nt_user_call_next_hook_ex, 4);
    register!("NtUserHideCaret", nt_user_hide_caret, 1);
    register!("NtUserCallHwndLock", nt_user_call_hwnd_lock, 2);
    register!(
        "NtUserGetProcessWindowStation",
        nt_user_get_process_window_station,
        0
    );
    register!("NtGdiDeleteObjectApp", nt_gdi_delete_object, 1);
    register!("NtUserSetWindowPos", nt_user_set_window_pos, 7);
    register!("NtUserShowCaret", nt_user_show_caret, 1);
    register!(
        "NtUserEndDeferWindowPosEx",
        nt_user_end_defer_window_pos_ex,
        2
    );
    register!("NtUserCallHwndParamLock", nt_user_call_hwnd_param_lock, 3);
    register!("NtUserVkKeyScanEx", nt_user_vk_key_scan_ex, 3);
    register!("NtUserCallTwoParam", nt_user_call_two_param, 3);
    register!(
        "NtUserCopyAcceleratorTable",
        nt_user_copy_accelerator_table,
        3
    );
    register!("NtUserNotifyWinEvent", nt_user_notify_win_event, 4);
    register!(
        "NtUserIsClipboardFormatAvailable",
        nt_user_is_clipboard_format_available,
        1
    );
    register!("NtUserSetScrollInfo", nt_user_set_scroll_info, 4);
    register!("NtGdiStretchBlt", nt_gdi_stretch_blt, 12);
    register!("NtUserCreateCaret", nt_user_create_caret, 4);
    register!("NtGdiCombineRgn", nt_gdi_combine_rgn, 4);
    register!("NtGdiGetDCObject", nt_gdi_get_dc_object, 2);
    register!("NtUserDispatchMessage", nt_user_dispatch_message, 1);
    register!(
        "NtUserRegisterWindowMessage",
        nt_user_register_window_message,
        1
    );
    register!("NtGdiExtTextOutW", nt_gdi_ext_text_out, 9);
    register!("NtGdiSelectFont", nt_gdi_select_font, 2);
    register!("NtGdiRestoreDC", nt_gdi_restore_dc, 2);
    register!("NtGdiSaveDC", nt_gdi_save_dc, 1);
    register!(
        "NtUserGetForegroundWindow",
        nt_user_get_foreground_window,
        0
    );
    register!("NtUserShowScrollBar", nt_user_show_scroll_bar, 3);
    register!(
        "NtUserFindExistingCursorIcon",
        nt_user_find_existing_cursor_icon,
        3
    );
    register!("NtGdiGetDCDword", nt_gdi_get_dc_dword, 3);
    register!("NtGdiGetAndSetDCDword", nt_gdi_get_and_set_dc_dword, 4);
    register!("NtGdiScaleViewportExtEx", nt_gdi_scale_viewport_ext_ex, 6);
    register!("NtGdiScaleWindowExtEx", nt_gdi_scale_window_ext_ex, 6);
    register!("NtGdiLineTo", nt_gdi_line_to, 3);
    register!(
        "NtUserSystemParametersInfo",
        nt_user_system_parameters_info,
        4
    );
    register!("NtGdiGetAppClipBox", nt_gdi_get_app_clip_box, 2);
    register!("NtUserGetAsyncKeyState", nt_user_get_async_key_state, 1);
    register!("NtUserRemoveProp", nt_user_remove_prop, 2);
    register!("NtGdiDoPalette", nt_gdi_do_palette, 6);
    register!("NtUserSetCapture", nt_user_set_capture, 1);
    register!(
        "NtGdiCreateCompatibleBitmap",
        nt_gdi_create_compatible_bitmap,
        3
    );
    register!("NtUserSetProp", nt_user_set_prop, 3);
    register!("NtGdiGetTextCharsetInfo", nt_gdi_get_text_charset_info, 3);
    register!("NtUserSBGetParms", nt_user_sb_get_parms, 4);
    register!("NtUserGetIconInfo", nt_user_get_icon_info, 6);
    register!("NtUserExcludeUpdateRgn", nt_user_exclude_update_rgn, 2);
    register!("NtUserSetFocus", nt_user_set_focus, 1);
    register!("NtGdiExtGetObjectW", nt_gdi_ext_get_object_w, 3);
    register!("NtUserDeferWindowPos", nt_user_defer_window_pos, 8);
    register!("NtUserGetUpdateRect", nt_user_get_update_rect, 3);
    register!("NtGdiCreateCompatibleDC", nt_gdi_create_compatible_dc, 1);
    register!(
        "NtUserGetClipboardSequenceNumber",
        nt_user_get_clipboard_sequence_number,
        0
    );
    register!("NtGdiCreatePen", nt_gdi_create_pen, 4);
    register!("NtUserShowWindow", nt_user_show_window, 2);
    register!(
        "NtUserGetKeyboardLayoutList",
        nt_user_get_keyboard_layout_list,
        2
    );
    register!("NtGdiPatBlt", nt_gdi_pat_blt, 6);
    register!("NtUserMapVirtualKeyEx", nt_user_map_virtual_key_ex, 4);
    register!("NtUserSetWindowLong", nt_user_set_window_long, 4);
    register!("NtGdiHfontCreate", nt_gdi_hfont_create, 5);
    register!("NtUserMoveWindow", nt_user_move_window, 6);
    register!("NtUserPostThreadMessage", nt_user_post_thread_message, 4);
    register!("NtUserDrawIconEx", nt_user_draw_icon_ex, 11);
    register!("NtUserGetSystemMenu", nt_user_get_system_menu, 2);
    register!("NtGdiDrawStream", nt_gdi_draw_stream, 3);
    register!(
        "NtUserInternalGetWindowText",
        nt_user_internal_get_window_text,
        3
    );
    register!("NtUserGetWindowDC", nt_user_get_window_dc, 1);
    register!("NtGdiInvertRgn", nt_gdi_invert_rgn, 2);
    register!("NtGdiGetRgnBox", nt_gdi_get_rgn_box, 2);
    register!("NtGdiMaskBlt", nt_gdi_mask_blt, 13);
    register!("NtGdiGetWidthTable", nt_gdi_get_width_table, 7);
    register!("NtUserScrollDC", nt_user_scroll_dc, 7);
    register!(
        "NtUserGetObjectInformation",
        nt_user_get_object_information,
        5
    );
    register!("NtGdiCreateBitmap", nt_gdi_create_bitmap, 5);
    register!("NtUserFindWindowEx", nt_user_find_window_ex, 5);
    register!("NtGdiPolyPatBlt", nt_gdi_poly_pat_blt, 5);
    register!(
        "NtUserUnhookWindowsHookEx",
        nt_user_unhook_windows_hook_ex,
        1
    );
    register!("NtGdiGetNearestColor", nt_gdi_get_nearest_color, 2);
    register!("NtGdiTransformPoints", nt_gdi_transform_points, 5);
    register!("NtGdiCombineTransform", nt_gdi_combine_transform, 3);
    register!("NtGdiGetTransform", nt_gdi_get_transform, 3);
    register!("NtGdiMoveTo", nt_gdi_move_to, 4);
    register!("NtGdiBeginPath", nt_gdi_begin_path, 1);
    register!("NtGdiEndPath", nt_gdi_end_path, 1);
    register!("NtGdiAbortPath", nt_gdi_abort_path, 1);
    register!("NtGdiCloseFigure", nt_gdi_close_figure, 1);
    register!("NtGdiFlattenPath", nt_gdi_flatten_path, 1);
    register!("NtGdiGetPath", nt_gdi_get_path, 4);
    register!("NtGdiFillPath", nt_gdi_fill_path, 1);
    register!("NtGdiStrokePath", nt_gdi_stroke_path, 1);
    register!("NtGdiStrokeAndFillPath", nt_gdi_stroke_and_fill_path, 1);
    register!("NtGdiPathToRegion", nt_gdi_path_to_region, 1);
    register!("NtGdiSelectClipPath", nt_gdi_select_clip_path, 2);
    register!("NtGdiPolyDraw", nt_gdi_poly_draw, 4);
    register!("NtGdiPolyPolyDraw", nt_gdi_poly_poly_draw, 5);
    register!("NtGdiGetMiterLimit", nt_gdi_get_miter_limit, 2);
    register!("NtGdiSetMiterLimit", nt_gdi_set_miter_limit, 3);
    register!("NtGdiSelectPen", nt_gdi_select_pen, 2);
    register!("NtGdiSelectBrush", nt_gdi_select_brush, 2);
    register!("NtGdiGetDCPoint", nt_gdi_get_dc_point, 3);
    register!("NtGdiCreateDIBBrush", nt_gdi_create_dib_brush, 6);
    register!("NtGdiGetTextMetricsW", nt_gdi_get_text_metrics_w, 3);
    register!("NtUserCreateWindowEx", nt_user_create_window_ex, 15);
    register!("NtUserSetParent", nt_user_set_parent, 2);
    register!("NtUserGetKeyboardState", nt_user_get_keyboard_state, 1);
    register!("NtUserToUnicodeEx", nt_user_to_unicode_ex, 7);
    register!("NtUserGetControlBrush", nt_user_get_control_brush, 3);
    register!("NtUserGetClassName", nt_user_get_class_name, 3);
    register!("NtGdiAlphaBlend", nt_gdi_alpha_blend, 12);
    register!("NtGdiDdBlt", nt_gdi_dd_blt, 3);
    register!("NtGdiOffsetRgn", nt_gdi_offset_rgn, 3);
    register!("NtUserDefSetText", nt_user_def_set_text, 2);
    register!("NtGdiGetTextFaceW", nt_gdi_get_text_face_w, 4);
    register!(
        "NtGdiStretchDIBitsInternal",
        nt_gdi_stretch_dibits_internal,
        16
    );
    register!("NtUserSendInput", nt_user_send_input, 3);
    register!("NtUserGetThreadDesktop", nt_user_get_thread_desktop, 2);
    register!("NtGdiCreateRectRgn", nt_gdi_create_rect_rgn, 4);
    register!("NtGdiGetDIBitsInternal", nt_gdi_get_dibits_internal, 9);
    register!("NtUserGetUpdateRgn", nt_user_get_update_rgn, 3);
    register!("NtGdiDeleteClientObj", nt_gdi_delete_client_obj, 1);
    register!("NtUserGetIconSize", nt_user_get_icon_size, 4);
    register!("NtUserFillWindow", nt_user_fill_window, 4);
    register!("NtGdiExtCreateRegion", nt_gdi_ext_create_region, 3);
    register!(
        "NtGdiComputeXformCoefficients",
        nt_gdi_compute_xform_coefficients,
        1
    );
    register!("NtUserSetWindowsHookEx", nt_user_set_windows_hook_ex, 6);
    register!(
        "NtUserNotifyProcessCreate",
        nt_user_notify_process_create,
        4
    );
    register!("NtGdiUnrealizeObject", nt_gdi_unrealize_object, 1);
    register!("NtUserGetTitleBarInfo", nt_user_get_title_bar_info, 2);
    register!("NtGdiRectangle", nt_gdi_rectangle, 5);
    register!("NtUserSetThreadDesktop", nt_user_set_thread_desktop, 1);
    register!("NtUserGetDCEx", nt_user_get_dcex, 3);
    register!("NtUserGetScrollBarInfo", nt_user_get_scroll_bar_info, 3);
    register!("NtGdiGetTextExtent", nt_gdi_get_text_extent, 5);
    register!("NtUserSetWindowFNID", nt_user_set_window_fnid, 2);
    register!("NtGdiSetLayout", nt_gdi_set_layout, 3);
    register!("NtUserCalcMenuBar", nt_user_calc_menu_bar, 5);
    register!(
        "NtUserThunkedMenuItemInfo",
        nt_user_thunked_menu_item_info,
        6
    );
    register!("NtGdiExcludeClipRect", nt_gdi_exclude_clip_rect, 5);
    register!("NtGdiCreateDIBSection", nt_gdi_create_dib_section, 9);
    register!("NtGdiGetDCforBitmap", nt_gdi_get_dc_for_bitmap, 1);
    register!("NtUserDestroyCursor", nt_user_destroy_cursor, 2);
    register!("NtUserDestroyWindow", nt_user_destroy_window, 1);
    register!("NtUserCallHwndParam", nt_user_call_hwnd_param, 3);
    register!(
        "NtGdiCreateDIBitmapInternal",
        nt_gdi_create_dibitmap_internal,
        11
    );
    register!("NtUserOpenWindowStation", nt_user_open_window_station, 2);
    register!(
        "NtGdiDdDeleteSurfaceObject",
        nt_gdi_dd_delete_surface_object,
        1
    );
    register!("NtGdiDdCanCreateSurface", nt_gdi_dd_can_create_surface, 2);
    register!("NtGdiDdCreateSurface", nt_gdi_dd_create_surface, 8);
    register!("NtUserSetCursorIconData", nt_user_set_cursor_icon_data, 4);
    register!("NtGdiDdDestroySurface", nt_gdi_dd_destroy_surface, 2);
    register!("NtUserCloseDesktop", nt_user_close_desktop, 1);
    register!("NtUserOpenDesktop", nt_user_open_desktop, 3);
    register!(
        "NtUserSetProcessWindowStation",
        nt_user_set_process_window_station,
        1
    );
    register!("NtUserGetAtomName", nt_user_get_atom_name, 2);
    register!("NtGdiDdResetVisrgn", nt_gdi_dd_reset_visrgn, 2);
    register!("NtGdiExtCreatePen", nt_gdi_ext_create_pen, 11);
    register!(
        "NtGdiCreatePaletteInternal",
        nt_gdi_create_palette_internal,
        2
    );
    register!("NtGdiSetBrushOrg", nt_gdi_set_brush_org, 4);
    register!("NtUserBuildNameList", nt_user_build_name_list, 4);
    register!("NtGdiSetPixel", nt_gdi_set_pixel, 4);
    register!("NtUserRegisterClassExWOW", nt_user_register_class_ex_wow, 7);
    register!(
        "NtGdiCreatePatternBrushInternal",
        nt_gdi_create_pattern_brush_internal,
        3
    );
    register!("NtUserGetAncestor", nt_user_get_ancestor, 2);
    register!(
        "NtGdiGetOutlineTextMetricsInternalW",
        nt_gdi_get_outline_text_metrics_internal_w,
        4
    );
    register!("NtGdiSetBitmapBits", nt_gdi_set_bitmap_bits, 3);
    register!("NtUserCloseWindowStation", nt_user_close_window_station, 1);
    register!("NtUserGetDoubleClickTime", nt_user_get_double_click_time, 0);
    register!("NtUserEnableScrollBar", nt_user_enable_scroll_bar, 3);
    register!("NtGdiCreateSolidBrush", nt_gdi_create_solid_brush, 2);
    register!("NtUserGetClassInfoEx", nt_user_get_class_info_ex, 5);
    register!("NtGdiCreateClientObj", nt_gdi_create_client_obj, 1);
    register!("NtUserUnregisterClass", nt_user_unregister_class, 3);
    register!("NtUserDeleteMenu", nt_user_delete_menu, 3);
    register!("NtGdiRectInRegion", nt_gdi_rect_in_region, 2);
    register!("NtUserScrollWindowEx", nt_user_scroll_window_ex, 8);
    register!("NtGdiGetPixel", nt_gdi_get_pixel, 3);
    register!("NtUserSetClassLong", nt_user_set_class_long, 4);
    register!("NtUserGetMenuBarInfo", nt_user_get_menu_bar_info, 4);
    register!("NtUserInvalidateRgn", nt_user_invalidate_rgn, 3);
    register!("NtUserGetClipboardOwner", nt_user_get_clipboard_owner, 0);
    register!("NtUserSetWindowRgn", nt_user_set_window_rgn, 3);
    register!("NtUserBitBltSysBmp", nt_user_bit_blt_sys_bmp, 8);
    register!("NtUserValidateRect", nt_user_validate_rect, 2);
    register!("NtUserCloseClipboard", nt_user_close_clipboard, 0);
    register!("NtUserOpenClipboard", nt_user_open_clipboard, 2);
    register!("NtUserSetClipboardData", nt_user_set_clipboard_data, 3);
    register!("NtUserEnableMenuItem", nt_user_enable_menu_item, 3);
    register!("NtUserAlterWindowStyle", nt_user_alter_window_style, 3);
    register!("NtGdiFillRgn", nt_gdi_fill_rgn, 3);
    register!("NtUserGetWindowPlacement", nt_user_get_window_placement, 2);
    register!(
        "NtGdiModifyWorldTransform",
        nt_gdi_modify_world_transform,
        3
    );
    register!(
        "NtUserGetOpenClipboardWindow",
        nt_user_get_open_clipboard_window,
        0
    );
    register!("NtUserSetThreadState", nt_user_set_thread_state, 2);
    register!("NtGdiOpenDCW", nt_gdi_open_dc_w, 8);
    register!("NtUserTrackMouseEvent", nt_user_track_mouse_event, 1);
    register!("NtUserDestroyMenu", nt_user_destroy_menu, 1);
    register!("NtGdiGetBitmapBits", nt_gdi_get_bitmap_bits, 3);
    register!("NtGdiGetBitmapDimension", nt_gdi_get_bitmap_dimension, 2);
    register!("NtGdiSetBitmapDimension", nt_gdi_set_bitmap_dimension, 4);
    register!("NtUserConsoleControl", nt_user_console_control, 3);
    register!("NtUserSetActiveWindow", nt_user_set_active_window, 1);
    register!(
        "NtUserSetInformationThread",
        nt_user_set_information_thread,
        4
    );
    register!("NtUserSetWindowPlacement", nt_user_set_window_placement, 2);
    register!("NtUserGetControlColor", nt_user_get_control_color, 4);
    register!("NtUserSetWindowWord", nt_user_set_window_word, 3);
    register!(
        "NtUserGetClipboardFormatName",
        nt_user_get_clipboard_format_name,
        3
    );
    register!(
        "NtUserRealInternalGetMessage",
        nt_user_real_internal_get_message,
        6
    );
    register!(
        "NtUserCreateLocalMemHandle",
        nt_user_create_local_mem_handle,
        4
    );
    register!("NtUserAttachThreadInput", nt_user_attach_thread_input, 3);
    register!("NtUserPaintMenuBar", nt_user_paint_menu_bar, 6);
    register!("NtUserSetKeyboardState", nt_user_set_keyboard_state, 1);
    register!(
        "NtUserCreateAcceleratorTable",
        nt_user_create_accelerator_table,
        2
    );
    register!("NtUserGetCursorFrameInfo", nt_user_get_cursor_frame_info, 4);
    register!("NtUserGetAltTabInfo", nt_user_get_alt_tab_info, 6);
    register!("NtUserGetCaretBlinkTime", nt_user_get_caret_blink_time, 0);
    register!("NtUserProcessConnect", nt_user_process_connect, 3);
    register!("NtUserEnumDisplayDevices", nt_user_enum_display_devices, 4);
    register!("NtUserEmptyClipboard", nt_user_empty_clipboard, 0);
    register!("NtUserGetClipboardData", nt_user_get_clipboard_data, 2);
    register!("NtUserRemoveMenu", nt_user_remove_menu, 3);
    register!("NtUserConvertMemHandle", nt_user_convert_mem_handle, 2);
    register!(
        "NtUserDestroyAcceleratorTable",
        nt_user_destroy_accelerator_table,
        1
    );
    register!("NtUserGetGUIThreadInfo", nt_user_get_gui_thread_info, 2);
    register!("NtUserSetWindowsHookAW", nt_user_set_windows_hook_aw, 3);
    register!("NtUserSetMenuDefaultItem", nt_user_set_menu_default_item, 3);
    register!("NtUserCheckMenuItem", nt_user_check_menu_item, 3);
    register!("NtUserSetWinEventHook", nt_user_set_win_event_hook, 8);
    register!("NtUserUnhookWinEvent", nt_user_unhook_win_event, 1);
    register!("NtUserLockWindowUpdate", nt_user_lock_window_update, 1);
    register!("NtUserSetSystemMenu", nt_user_set_system_menu, 2);
    register!("NtUserThunkedMenuInfo", nt_user_thunked_menu_info, 2);
    register!("NtUserCallHwnd", nt_user_call_hwnd, 2);
    register!("NtUserDdeInitialize", nt_user_dde_initialize, 5);
    register!(
        "NtUserModifyUserStartupInfoFlags",
        nt_user_modify_user_startup_info_flags,
        2
    );
    register!(
        "NtUserCountClipboardFormats",
        nt_user_count_clipboard_formats,
        0
    );
    register!(
        "NtUserEnumDisplaySettings",
        nt_user_enum_display_settings,
        4
    );
    register!("NtUserPaintDesktop", nt_user_paint_desktop, 1);
    register!(
        "NtUserChangeClipboardChain",
        nt_user_change_clipboard_chain,
        2
    );
    register!("NtUserSetClipboardViewer", nt_user_set_clipboard_viewer, 1);
    register!("NtUserShowWindowAsync", nt_user_show_window_async, 2);
    register!(
        "NtUserActivateKeyboardLayout",
        nt_user_activate_keyboard_layout,
        2
    );
    register!("NtUserEndMenu", nt_user_end_menu, 0);
    register!("NtUserGetCursorInfo", nt_user_get_cursor_info, 1);
    register!(
        "NtUserInitializeClientPfnArrays",
        nt_user_initialize_client_pfn_arrays,
        4
    );
    register!("NtUserRegisterHotKey", nt_user_register_hot_key, 4);
    register!("NtUserSetMenu", nt_user_set_menu, 3);
    register!("NtUserUnregisterHotKey", nt_user_unregister_hot_key, 2);
}

/// Register a shadow (win32k) service handler
///
/// # Safety
/// This function modifies a static mutable array. It should only be
/// called during initialization when no other cores are running.
pub unsafe fn register_shadow_service(service_index: u32, handler: *const (), arg_size: u8) {
    if service_index as usize >= SHADOW_MAX_SERVICES {
        // // kprintln!("[SHADOW SSDT] ERROR: Service index 0x{:03x} out of range", service_index);
        return;
    }

    SHADOW_SERVICE_TABLE[service_index as usize].handler = handler;
    SHADOW_ARGUMENT_TABLE[service_index as usize] = arg_size;

    // // kprintln!("[SHADOW SSDT] Registered shadow service 0x{:04x} at index {} (args={})",
    // //               service_index, service_index, arg_size);
}

/// Get a shadow service handler
pub fn get_shadow_service(service_index: u32) -> Option<*const ()> {
    if service_index as usize >= SHADOW_MAX_SERVICES {
        return None;
    }

    unsafe {
        let entry = &SHADOW_SERVICE_TABLE[service_index as usize];
        if entry.handler.is_null() {
            None
        } else {
            Some(entry.handler)
        }
    }
}

/// Get the argument byte count for a shadow (win32k) service
pub fn get_argument_size(service_index: u32) -> u8 {
    if service_index as usize >= SHADOW_MAX_SERVICES {
        return 0;
    }
    unsafe { SHADOW_ARGUMENT_TABLE[service_index as usize] }
}

/// Get a pointer to the shadow service table (for KeServiceDescriptorTable)
pub fn service_table_ptr() -> *const () {
    core::ptr::addr_of!(SHADOW_SERVICE_TABLE) as *const ()
}

/// Get a pointer to the shadow argument table (for KeServiceDescriptorTable)
pub fn argument_table_ptr() -> *const u8 {
    core::ptr::addr_of!(SHADOW_ARGUMENT_TABLE) as *const u8
}

// =====================================================================
// Win32k Syscall Numbers - Windows 7 SP1 x64
// Based on j00ru/windows-syscalls project
// =====================================================================

// ----------------------------------------------------------------------------
// GDI Services - Graphics Device Interface
// ----------------------------------------------------------------------------

pub const NT_GDI_BIT_BLT: u32 = 0x1008;
pub const NT_GDI_GET_CHAR_SET: u32 = 0x1009;
pub const NT_GDI_SELECT_BITMAP: u32 = 0x100b;
pub const NT_GDI_FLUSH: u32 = 0x1012;
pub const NT_GDI_INTERSECT_CLIP_RECT: u32 = 0x1020;
pub const NT_GDI_DELETE_OBJECT_APP: u32 = 0x1023;
pub const NT_GDI_SET_DIBITS_TO_DEVICE_INTERNAL: u32 = 0x1029;
pub const NT_GDI_GET_RANDOM_RGN: u32 = 0x102b;
pub const NT_GDI_EXT_SELECT_CLIP_RGN: u32 = 0x102e;
pub const NT_GDI_STRETCH_BLT: u32 = 0x1031;
pub const NT_GDI_RECT_VISIBLE: u32 = 0x1033;
pub const NT_GDI_COMBINE_RGN: u32 = 0x1034;
pub const NT_GDI_GET_DCOBJECT: u32 = 0x1035;
pub const NT_GDI_EXT_TEXT_OUT_W: u32 = 0x1038;
pub const NT_GDI_SELECT_FONT: u32 = 0x1039;
pub const NT_GDI_RESTORE_DC: u32 = 0x103a;
pub const NT_GDI_SAVE_DC: u32 = 0x103b;
pub const NT_GDI_GET_DCDWORD: u32 = 0x103f;
pub const NT_GDI_GET_REGION_DATA: u32 = 0x1040;
pub const NT_GDI_LINE_TO: u32 = 0x1041;
pub const NT_GDI_GET_APP_CLIP_BOX: u32 = 0x1043;
pub const NT_GDI_DO_PALETTE: u32 = 0x1047;
pub const NT_GDI_POLY_POLY_DRAW: u32 = 0x1048;
pub const NT_GDI_CREATE_COMPATIBLE_BITMAP: u32 = 0x104b;
pub const NT_GDI_GET_TEXT_CHARSET_INFO: u32 = 0x104d;
pub const NT_GDI_EXT_GET_OBJECT_W: u32 = 0x1052;
pub const NT_GDI_CREATE_COMPATIBLE_DC: u32 = 0x1055;
pub const NT_GDI_CREATE_PEN: u32 = 0x1057;
pub const NT_GDI_PAT_BLT: u32 = 0x105a;
pub const NT_GDI_HFONT_CREATE: u32 = 0x105d;
pub const NT_GDI_DRAW_STREAM: u32 = 0x1062;
pub const NT_GDI_D3D_DRAW_PRIMITIVES2: u32 = 0x1065;
pub const NT_GDI_INVERT_RGN: u32 = 0x1066;
pub const NT_GDI_GET_RGN_BOX: u32 = 0x1067;
pub const NT_GDI_GET_AND_SET_DCDWORD: u32 = 0x1068;
pub const NT_GDI_MASK_BLT: u32 = 0x1069;
pub const NT_GDI_GET_WIDTH_TABLE: u32 = 0x106a;
pub const NT_GDI_CREATE_BITMAP: u32 = 0x106d;
pub const NT_GDI_POLY_PAT_BLT: u32 = 0x106f;
pub const NT_GDI_GET_NEAREST_COLOR: u32 = 0x1071;
pub const NT_GDI_TRANSFORM_POINTS: u32 = 0x1072;
pub const NT_GDI_GET_DCPOINT: u32 = 0x1073;
pub const NT_GDI_CREATE_DIBBRUSH: u32 = 0x1074;
pub const NT_GDI_GET_TEXT_METRICS_W: u32 = 0x1075;
pub const NT_GDI_ALPHA_BLEND: u32 = 0x107c;
pub const NT_GDI_DD_BLT: u32 = 0x107d;
pub const NT_GDI_OFFSET_RGN: u32 = 0x107e;
pub const NT_GDI_GET_TEXT_FACE_W: u32 = 0x1080;
pub const NT_GDI_STRETCH_DIBITS_INTERNAL: u32 = 0x1081;
pub const NT_GDI_CREATE_RECT_RGN: u32 = 0x1084;
pub const NT_GDI_GET_DIBITS_INTERNAL: u32 = 0x1085;
pub const NT_GDI_DELETE_CLIENT_OBJ: u32 = 0x1087;
pub const NT_GDI_EXT_CREATE_REGION: u32 = 0x108a;
pub const NT_GDI_COMPUTE_XFORM_COEFFICIENTS: u32 = 0x108b;
pub const NT_GDI_UNREALIZE_OBJECT: u32 = 0x108e;
pub const NT_GDI_RECTANGLE: u32 = 0x1090;
pub const NT_GDI_GET_TEXT_EXTENT: u32 = 0x1094;
pub const NT_GDI_SET_LAYOUT: u32 = 0x1096;
pub const NT_GDI_EXCLUDE_CLIP_RECT: u32 = 0x1099;
pub const NT_GDI_CREATE_DIBSECTION: u32 = 0x109a;
pub const NT_GDI_GET_DCFOR_BITMAP: u32 = 0x109b;
pub const NT_GDI_CREATE_DIBITMAP_INTERNAL: u32 = 0x109f;
pub const NT_GDI_DD_DELETE_SURFACE_OBJECT: u32 = 0x10a1;
pub const NT_GDI_DD_CAN_CREATE_SURFACE: u32 = 0x10a2;
pub const NT_GDI_DD_CREATE_SURFACE: u32 = 0x10a3;
pub const NT_GDI_DD_DESTROY_SURFACE: u32 = 0x10a5;
pub const NT_GDI_DD_RESET_VISRGN: u32 = 0x10aa;
pub const NT_GDI_EXT_CREATE_PEN: u32 = 0x10ab;
pub const NT_GDI_CREATE_PALETTE_INTERNAL: u32 = 0x10ac;
pub const NT_GDI_SET_BRUSH_ORG: u32 = 0x10ad;
pub const NT_GDI_SET_PIXEL: u32 = 0x10af;
pub const NT_GDI_CREATE_PATTERN_BRUSH_INTERNAL: u32 = 0x10b1;
pub const NT_GDI_GET_OUTLINE_TEXT_METRICS_INTERNAL_W: u32 = 0x10b3;
pub const NT_GDI_SET_BITMAP_BITS: u32 = 0x10b4;
pub const NT_GDI_CREATE_SOLID_BRUSH: u32 = 0x10b8;
pub const NT_GDI_CREATE_CLIENT_OBJ: u32 = 0x10ba;
pub const NT_GDI_RECT_IN_REGION: u32 = 0x10bd;
pub const NT_GDI_GET_PIXEL: u32 = 0x10bf;
pub const NT_GDI_DD_CREATE_SURFACE_EX: u32 = 0x10c2;
pub const NT_GDI_DD_CREATE_SURFACE_OBJECT: u32 = 0x10c3;
pub const NT_GDI_GET_NEAREST_PALETTE_INDEX: u32 = 0x10c4;
pub const NT_GDI_DD_LOCK_D3_D: u32 = 0x10c5;
pub const NT_GDI_DD_UNLOCK_D3_D: u32 = 0x10c6;
pub const NT_GDI_GET_CHAR_WIDTH_W: u32 = 0x10c7;
pub const NT_GDI_GET_CHAR_WIDTH_INFO: u32 = 0x10cc;
pub const NT_GDI_GET_STOCK_OBJECT: u32 = 0x10d0;
pub const NT_GDI_FILL_RGN: u32 = 0x10d4;
pub const NT_GDI_MODIFY_WORLD_TRANSFORM: u32 = 0x10d6;
pub const NT_GDI_GET_FONT_DATA: u32 = 0x10d7;
pub const NT_GDI_OPEN_DCW: u32 = 0x10da;
pub const NT_GDI_GET_TRANSFORM: u32 = 0x10dc;
pub const NT_GDI_GET_BITMAP_BITS: u32 = 0x10de;
pub const NT_GDI_SET_META_RGN: u32 = 0x10e4;
pub const NT_GDI_SET_MITER_LIMIT: u32 = 0x10e5;
pub const NT_GDI_SET_VIRTUAL_RESOLUTION: u32 = 0x10e6;
pub const NT_GDI_GET_RASTERIZER_CAPS: u32 = 0x10e7;
pub const NT_GDI_CREATE_HALFTONE_PALETTE: u32 = 0x10ed;
pub const NT_GDI_COMBINE_TRANSFORM: u32 = 0x10f0;
pub const NT_GDI_QUERY_FONT_ASSOC_INFO: u32 = 0x10f5;
pub const NT_GDI_SET_BOUNDS_RECT: u32 = 0x10fb;
pub const NT_GDI_GET_BITMAP_DIMENSION: u32 = 0x10fc;
pub const NT_GDI_CLOSE_FIGURE: u32 = 0x1100;
pub const NT_GDI_BEGIN_PATH: u32 = 0x1109;
pub const NT_GDI_END_PATH: u32 = 0x110a;
pub const NT_GDI_FILL_PATH: u32 = 0x110b;
pub const NT_GDI_ADD_FONT_MEM_RESOURCE_EX: u32 = 0x1110;
pub const NT_GDI_EQUAL_RGN: u32 = 0x1111;
pub const NT_GDI_GET_SYSTEM_PALETTE_USE: u32 = 0x1112;
pub const NT_GDI_REMOVE_FONT_MEM_RESOURCE_EX: u32 = 0x1113;
pub const NT_GDI_EXT_ESCAPE: u32 = 0x1116;
pub const NT_GDI_SET_BITMAP_DIMENSION: u32 = 0x1117;
pub const NT_GDI_SET_FONT_ENUMERATION: u32 = 0x1118;
pub const NT_GDI_CREATE_COLOR_SPACE: u32 = 0x111c;
pub const NT_GDI_DELETE_COLOR_SPACE: u32 = 0x111d;
pub const NT_GDI_ABORT_DOC: u32 = 0x111f;
pub const NT_GDI_ABORT_PATH: u32 = 0x1120;
pub const NT_GDI_ADD_EMB_FONT_TO_DC: u32 = 0x1121;
pub const NT_GDI_ADD_FONT_RESOURCE_W: u32 = 0x1122;
pub const NT_GDI_ADD_REMOTE_FONT_TO_DC: u32 = 0x1123;
pub const NT_GDI_ADD_REMOTE_MMINSTANCE_TO_DC: u32 = 0x1124;
pub const NT_GDI_ANGLE_ARC: u32 = 0x1125;
pub const NT_GDI_ANY_LINKED_FONTS: u32 = 0x1126;
pub const NT_GDI_ARC_INTERNAL: u32 = 0x1127;
pub const NT_GDI_BRUSHOBJ_DELETE_RBRUSH: u32 = 0x1128;
pub const NT_GDI_BRUSHOBJ_H_GET_COLOR_TRANSFORM: u32 = 0x1129;
pub const NT_GDI_BRUSHOBJ_PV_ALLOC_RBRUSH: u32 = 0x112a;
pub const NT_GDI_BRUSHOBJ_PV_GET_RBRUSH: u32 = 0x112b;
pub const NT_GDI_BRUSHOBJ_UL_GET_BRUSH_COLOR: u32 = 0x112c;
pub const NT_GDI_BEGIN_GDI_RENDERING: u32 = 0x112d;
pub const NT_GDI_CLIPOBJ_B_ENUM: u32 = 0x112e;
pub const NT_GDI_CLIPOBJ_C_ENUM_START: u32 = 0x112f;
pub const NT_GDI_CLIPOBJ_PPO_GET_PATH: u32 = 0x1130;
pub const NT_GDI_CANCEL_DC: u32 = 0x1131;
pub const NT_GDI_CHANGE_GHOST_FONT: u32 = 0x1132;
pub const NT_GDI_CHECK_BITMAP_BITS: u32 = 0x1133;
pub const NT_GDI_CLEAR_BITMAP_ATTRIBUTES: u32 = 0x1134;
pub const NT_GDI_CLEAR_BRUSH_ATTRIBUTES: u32 = 0x1135;
pub const NT_GDI_COLOR_CORRECT_PALETTE: u32 = 0x1136;
pub const NT_GDI_CONFIGURE_OPMPROTECTED_OUTPUT: u32 = 0x1137;
pub const NT_GDI_CONVERT_METAFILE_RECT: u32 = 0x1138;
pub const NT_GDI_CREATE_BITMAP_FROM_DX_SURFACE: u32 = 0x1139;
pub const NT_GDI_CREATE_COLOR_TRANSFORM: u32 = 0x113a;
pub const NT_GDI_CREATE_ELLIPTIC_RGN: u32 = 0x113b;
pub const NT_GDI_CREATE_HATCH_BRUSH_INTERNAL: u32 = 0x113c;
pub const NT_GDI_CREATE_METAFILE_DC: u32 = 0x113d;
pub const NT_GDI_CREATE_OPMPROTECTED_OUTPUTS: u32 = 0x113e;
pub const NT_GDI_CREATE_ROUND_RECT_RGN: u32 = 0x113f;
pub const NT_GDI_CREATE_SERVER_META_FILE: u32 = 0x1140;
pub const NT_GDI_D3D_CONTEXT_CREATE: u32 = 0x1141;
pub const NT_GDI_D3D_CONTEXT_DESTROY: u32 = 0x1142;
pub const NT_GDI_D3D_CONTEXT_DESTROY_ALL: u32 = 0x1143;
pub const NT_GDI_D3D_VALIDATE_TEXTURE_STAGE_STATE: u32 = 0x1144;
pub const NT_GDI_DDCCIGET_CAPABILITIES_STRING: u32 = 0x1145;
pub const NT_GDI_DDCCIGET_CAPABILITIES_STRING_LENGTH: u32 = 0x1146;
pub const NT_GDI_DDCCIGET_TIMING_REPORT: u32 = 0x1147;
pub const NT_GDI_DDCCIGET_VCPFEATURE: u32 = 0x1148;
pub const NT_GDI_DDCCISAVE_CURRENT_SETTINGS: u32 = 0x1149;
pub const NT_GDI_DDCCISET_VCPFEATURE: u32 = 0x114a;
pub const NT_GDI_DD_ADD_ATTACHED_SURFACE: u32 = 0x114b;
pub const NT_GDI_DD_ALPHA_BLT: u32 = 0x114c;
pub const NT_GDI_DD_ATTACH_SURFACE: u32 = 0x114d;
pub const NT_GDI_DD_BEGIN_MO_COMP_FRAME: u32 = 0x114e;
pub const NT_GDI_DD_CAN_CREATE_D3_DBUFFER: u32 = 0x114f;
pub const NT_GDI_DD_COLOR_CONTROL: u32 = 0x1150;
pub const NT_GDI_DD_CREATE_D3_DBUFFER: u32 = 0x1151;
pub const NT_GDI_DD_CREATE_DIRECT_DRAW_OBJECT: u32 = 0x1152;
pub const NT_GDI_DD_CREATE_FULLSCREEN_SPRITE: u32 = 0x1153;
pub const NT_GDI_DD_CREATE_MO_COMP: u32 = 0x1154;
pub const NT_GDI_DD_DDIACQUIRE_KEYED_MUTEX: u32 = 0x1155;
pub const NT_GDI_DD_DDICHECK_EXCLUSIVE_OWNERSHIP: u32 = 0x1156;
pub const NT_GDI_DD_DDICHECK_MONITOR_POWER_STATE: u32 = 0x1157;
pub const NT_GDI_DD_DDICHECK_OCCLUSION: u32 = 0x1158;
pub const NT_GDI_DD_DDICHECK_SHARED_RESOURCE_ACCESS: u32 = 0x1159;
pub const NT_GDI_DD_DDICHECK_VID_PN_EXCLUSIVE_OWNERSHIP: u32 = 0x115a;
pub const NT_GDI_DD_DDICLOSE_ADAPTER: u32 = 0x115b;
pub const NT_GDI_DD_DDICONFIGURE_SHARED_RESOURCE: u32 = 0x115c;
pub const NT_GDI_DD_DDICREATE_ALLOCATION: u32 = 0x115d;
pub const NT_GDI_DD_DDICREATE_CONTEXT: u32 = 0x115e;
pub const NT_GDI_DD_DDICREATE_DCFROM_MEMORY: u32 = 0x115f;
pub const NT_GDI_DD_DDICREATE_DEVICE: u32 = 0x1160;
pub const NT_GDI_DD_DDICREATE_KEYED_MUTEX: u32 = 0x1161;
pub const NT_GDI_DD_DDICREATE_OVERLAY: u32 = 0x1162;
pub const NT_GDI_DD_DDICREATE_SYNCHRONIZATION_OBJECT: u32 = 0x1163;
pub const NT_GDI_DD_DDIDESTROY_ALLOCATION: u32 = 0x1164;
pub const NT_GDI_DD_DDIDESTROY_CONTEXT: u32 = 0x1165;
pub const NT_GDI_DD_DDIDESTROY_DCFROM_MEMORY: u32 = 0x1166;
pub const NT_GDI_DD_DDIDESTROY_DEVICE: u32 = 0x1167;
pub const NT_GDI_DD_DDIDESTROY_KEYED_MUTEX: u32 = 0x1168;
pub const NT_GDI_DD_DDIDESTROY_OVERLAY: u32 = 0x1169;
pub const NT_GDI_DD_DDIDESTROY_SYNCHRONIZATION_OBJECT: u32 = 0x116a;
pub const NT_GDI_DD_DDIESCAPE: u32 = 0x116b;
pub const NT_GDI_DD_DDIFLIP_OVERLAY: u32 = 0x116c;
pub const NT_GDI_DD_DDIGET_CONTEXT_SCHEDULING_PRIORITY: u32 = 0x116d;
pub const NT_GDI_DD_DDIGET_DEVICE_STATE: u32 = 0x116e;
pub const NT_GDI_DD_DDIGET_DISPLAY_MODE_LIST: u32 = 0x116f;
pub const NT_GDI_DD_DDIGET_MULTISAMPLE_METHOD_LIST: u32 = 0x1170;
pub const NT_GDI_DD_DDIGET_OVERLAY_STATE: u32 = 0x1171;
pub const NT_GDI_DD_DDIGET_PRESENT_HISTORY: u32 = 0x1172;
pub const NT_GDI_DD_DDIGET_PRESENT_QUEUE_EVENT: u32 = 0x1173;
pub const NT_GDI_DD_DDIGET_PROCESS_SCHEDULING_PRIORITY_CLASS: u32 = 0x1174;
pub const NT_GDI_DD_DDIGET_RUNTIME_DATA: u32 = 0x1175;
pub const NT_GDI_DD_DDIGET_SCAN_LINE: u32 = 0x1176;
pub const NT_GDI_DD_DDIGET_SHARED_PRIMARY_HANDLE: u32 = 0x1177;
pub const NT_GDI_DD_DDIINVALIDATE_ACTIVE_VID_PN: u32 = 0x1178;
pub const NT_GDI_DD_DDILOCK: u32 = 0x1179;
pub const NT_GDI_DD_DDIOPEN_ADAPTER_FROM_DEVICE_NAME: u32 = 0x117a;
pub const NT_GDI_DD_DDIOPEN_ADAPTER_FROM_HDC: u32 = 0x117b;
pub const NT_GDI_DD_DDIOPEN_KEYED_MUTEX: u32 = 0x117c;
pub const NT_GDI_DD_DDIOPEN_RESOURCE: u32 = 0x117d;
pub const NT_GDI_DD_DDIOPEN_SYNCHRONIZATION_OBJECT: u32 = 0x117e;
pub const NT_GDI_DD_DDIPOLL_DISPLAY_CHILDREN: u32 = 0x117f;
pub const NT_GDI_DD_DDIPRESENT: u32 = 0x1180;
pub const NT_GDI_DD_DDIQUERY_ADAPTER_INFO: u32 = 0x1181;
pub const NT_GDI_DD_DDIQUERY_ALLOCATION_RESIDENCY: u32 = 0x1182;
pub const NT_GDI_DD_DDIQUERY_RESOURCE_INFO: u32 = 0x1183;
pub const NT_GDI_DD_DDIQUERY_STATISTICS: u32 = 0x1184;
pub const NT_GDI_DD_DDIRELEASE_KEYED_MUTEX: u32 = 0x1185;
pub const NT_GDI_DD_DDIRELEASE_PROCESS_VID_PN_SOURCE_OWNERS: u32 = 0x1186;
pub const NT_GDI_DD_DDIRENDER: u32 = 0x1187;
pub const NT_GDI_DD_DDISET_ALLOCATION_PRIORITY: u32 = 0x1188;
pub const NT_GDI_DD_DDISET_CONTEXT_SCHEDULING_PRIORITY: u32 = 0x1189;
pub const NT_GDI_DD_DDISET_DISPLAY_MODE: u32 = 0x118a;
pub const NT_GDI_DD_DDISET_DISPLAY_PRIVATE_DRIVER_FORMAT: u32 = 0x118b;
pub const NT_GDI_DD_DDISET_GAMMA_RAMP: u32 = 0x118c;
pub const NT_GDI_DD_DDISET_PROCESS_SCHEDULING_PRIORITY_CLASS: u32 = 0x118d;
pub const NT_GDI_DD_DDISET_QUEUED_LIMIT: u32 = 0x118e;
pub const NT_GDI_DD_DDISET_VID_PN_SOURCE_OWNER: u32 = 0x118f;
pub const NT_GDI_DD_DDISHARED_PRIMARY_LOCK_NOTIFICATION: u32 = 0x1190;
pub const NT_GDI_DD_DDISHARED_PRIMARY_UN_LOCK_NOTIFICATION: u32 = 0x1191;
pub const NT_GDI_DD_DDISIGNAL_SYNCHRONIZATION_OBJECT: u32 = 0x1192;
pub const NT_GDI_DD_DDIUNLOCK: u32 = 0x1193;
pub const NT_GDI_DD_DDIUPDATE_OVERLAY: u32 = 0x1194;
pub const NT_GDI_DD_DDIWAIT_FOR_IDLE: u32 = 0x1195;
pub const NT_GDI_DD_DDIWAIT_FOR_SYNCHRONIZATION_OBJECT: u32 = 0x1196;
pub const NT_GDI_DD_DDIWAIT_FOR_VERTICAL_BLANK_EVENT: u32 = 0x1197;
pub const NT_GDI_DD_DELETE_DIRECT_DRAW_OBJECT: u32 = 0x1198;
pub const NT_GDI_DD_DESTROY_D3_DBUFFER: u32 = 0x1199;
pub const NT_GDI_DD_DESTROY_FULLSCREEN_SPRITE: u32 = 0x119a;
pub const NT_GDI_DD_DESTROY_MO_COMP: u32 = 0x119b;
pub const NT_GDI_DD_END_MO_COMP_FRAME: u32 = 0x119c;
pub const NT_GDI_DD_FLIP: u32 = 0x119d;
pub const NT_GDI_DD_FLIP_TO_GDISURFACE: u32 = 0x119e;
pub const NT_GDI_DD_GET_AVAIL_DRIVER_MEMORY: u32 = 0x119f;
pub const NT_GDI_DD_GET_BLT_STATUS: u32 = 0x11a0;
pub const NT_GDI_DD_GET_DC: u32 = 0x11a1;
pub const NT_GDI_DD_GET_DRIVER_INFO: u32 = 0x11a2;
pub const NT_GDI_DD_GET_DRIVER_STATE: u32 = 0x11a3;
pub const NT_GDI_DD_GET_DX_HANDLE: u32 = 0x11a4;
pub const NT_GDI_DD_GET_FLIP_STATUS: u32 = 0x11a5;
pub const NT_GDI_DD_GET_INTERNAL_MO_COMP_INFO: u32 = 0x11a6;
pub const NT_GDI_DD_GET_MO_COMP_BUFF_INFO: u32 = 0x11a7;
pub const NT_GDI_DD_GET_MO_COMP_FORMATS: u32 = 0x11a8;
pub const NT_GDI_DD_GET_MO_COMP_GUIDS: u32 = 0x11a9;
pub const NT_GDI_DD_GET_SCAN_LINE: u32 = 0x11aa;
pub const NT_GDI_DD_LOCK: u32 = 0x11ab;
pub const NT_GDI_DD_NOTIFY_FULLSCREEN_SPRITE_UPDATE: u32 = 0x11ac;
pub const NT_GDI_DD_QUERY_DIRECT_DRAW_OBJECT: u32 = 0x11ad;
pub const NT_GDI_DD_QUERY_MO_COMP_STATUS: u32 = 0x11ae;
pub const NT_GDI_DD_QUERY_VIS_RGN_UNIQUENESS: u32 = 0x11af;
pub const NT_GDI_DD_REENABLE_DIRECT_DRAW_OBJECT: u32 = 0x11b0;
pub const NT_GDI_DD_RELEASE_DC: u32 = 0x11b1;
pub const NT_GDI_DD_RENDER_MO_COMP: u32 = 0x11b2;
pub const NT_GDI_DD_SET_COLOR_KEY: u32 = 0x11b3;
pub const NT_GDI_DD_SET_EXCLUSIVE_MODE: u32 = 0x11b4;
pub const NT_GDI_DD_SET_GAMMA_RAMP: u32 = 0x11b5;
pub const NT_GDI_DD_SET_OVERLAY_POSITION: u32 = 0x11b6;
pub const NT_GDI_DD_UNATTACH_SURFACE: u32 = 0x11b7;
pub const NT_GDI_DD_UNLOCK: u32 = 0x11b8;
pub const NT_GDI_DD_UPDATE_OVERLAY: u32 = 0x11b9;
pub const NT_GDI_DD_WAIT_FOR_VERTICAL_BLANK: u32 = 0x11ba;
pub const NT_GDI_DELETE_COLOR_TRANSFORM: u32 = 0x11bb;
pub const NT_GDI_DESCRIBE_PIXEL_FORMAT: u32 = 0x11bc;
pub const NT_GDI_DESTROY_OPMPROTECTED_OUTPUT: u32 = 0x11bd;
pub const NT_GDI_DESTROY_PHYSICAL_MONITOR: u32 = 0x11be;
pub const NT_GDI_DO_BANDING: u32 = 0x11bf;
pub const NT_GDI_DRAW_ESCAPE: u32 = 0x11c0;
pub const NT_GDI_DVP_ACQUIRE_NOTIFICATION: u32 = 0x11c1;
pub const NT_GDI_DVP_CAN_CREATE_VIDEO_PORT: u32 = 0x11c2;
pub const NT_GDI_DVP_COLOR_CONTROL: u32 = 0x11c3;
pub const NT_GDI_DVP_CREATE_VIDEO_PORT: u32 = 0x11c4;
pub const NT_GDI_DVP_DESTROY_VIDEO_PORT: u32 = 0x11c5;
pub const NT_GDI_DVP_FLIP_VIDEO_PORT: u32 = 0x11c6;
pub const NT_GDI_DVP_GET_VIDEO_PORT_BANDWIDTH: u32 = 0x11c7;
pub const NT_GDI_DVP_GET_VIDEO_PORT_CONNECT_INFO: u32 = 0x11c8;
pub const NT_GDI_DVP_GET_VIDEO_PORT_FIELD: u32 = 0x11c9;
pub const NT_GDI_DVP_GET_VIDEO_PORT_FLIP_STATUS: u32 = 0x11ca;
pub const NT_GDI_DVP_GET_VIDEO_PORT_INPUT_FORMATS: u32 = 0x11cb;
pub const NT_GDI_DVP_GET_VIDEO_PORT_LINE: u32 = 0x11cc;
pub const NT_GDI_DVP_GET_VIDEO_PORT_OUTPUT_FORMATS: u32 = 0x11cd;
pub const NT_GDI_DVP_GET_VIDEO_SIGNAL_STATUS: u32 = 0x11ce;
pub const NT_GDI_DVP_RELEASE_NOTIFICATION: u32 = 0x11cf;
pub const NT_GDI_DVP_UPDATE_VIDEO_PORT: u32 = 0x11d0;
pub const NT_GDI_DVP_WAIT_FOR_VIDEO_PORT_SYNC: u32 = 0x11d1;
pub const NT_GDI_DXG_GENERIC_THUNK: u32 = 0x11d2;
pub const NT_GDI_ELLIPSE: u32 = 0x11d3;
pub const NT_GDI_ENABLE_EUDC: u32 = 0x11d4;
pub const NT_GDI_END_DOC: u32 = 0x11d5;
pub const NT_GDI_END_GDI_RENDERING: u32 = 0x11d6;
pub const NT_GDI_END_PAGE: u32 = 0x11d7;
pub const NT_GDI_ENG_ALPHA_BLEND: u32 = 0x11d8;
pub const NT_GDI_ENG_ASSOCIATE_SURFACE: u32 = 0x11d9;
pub const NT_GDI_ENG_BIT_BLT: u32 = 0x11da;
pub const NT_GDI_ENG_CHECK_ABORT: u32 = 0x11db;
pub const NT_GDI_ENG_COMPUTE_GLYPH_SET: u32 = 0x11dc;
pub const NT_GDI_ENG_COPY_BITS: u32 = 0x11dd;
pub const NT_GDI_ENG_CREATE_BITMAP: u32 = 0x11de;
pub const NT_GDI_ENG_CREATE_CLIP: u32 = 0x11df;
pub const NT_GDI_ENG_CREATE_DEVICE_BITMAP: u32 = 0x11e0;
pub const NT_GDI_ENG_CREATE_DEVICE_SURFACE: u32 = 0x11e1;
pub const NT_GDI_ENG_CREATE_PALETTE: u32 = 0x11e2;
pub const NT_GDI_ENG_DELETE_CLIP: u32 = 0x11e3;
pub const NT_GDI_ENG_DELETE_PALETTE: u32 = 0x11e4;
pub const NT_GDI_ENG_DELETE_PATH: u32 = 0x11e5;
pub const NT_GDI_ENG_DELETE_SURFACE: u32 = 0x11e6;
pub const NT_GDI_ENG_ERASE_SURFACE: u32 = 0x11e7;
pub const NT_GDI_ENG_FILL_PATH: u32 = 0x11e8;
pub const NT_GDI_ENG_GRADIENT_FILL: u32 = 0x11e9;
pub const NT_GDI_ENG_LINE_TO: u32 = 0x11ea;
pub const NT_GDI_ENG_LOCK_SURFACE: u32 = 0x11eb;
pub const NT_GDI_ENG_MARK_BANDING_SURFACE: u32 = 0x11ec;
pub const NT_GDI_ENG_PAINT: u32 = 0x11ed;
pub const NT_GDI_ENG_PLG_BLT: u32 = 0x11ee;
pub const NT_GDI_ENG_STRETCH_BLT: u32 = 0x11ef;
pub const NT_GDI_ENG_STRETCH_BLT_ROP: u32 = 0x11f0;
pub const NT_GDI_ENG_STROKE_AND_FILL_PATH: u32 = 0x11f1;
pub const NT_GDI_ENG_STROKE_PATH: u32 = 0x11f2;
pub const NT_GDI_ENG_TEXT_OUT: u32 = 0x11f3;
pub const NT_GDI_ENG_TRANSPARENT_BLT: u32 = 0x11f4;
pub const NT_GDI_ENG_UNLOCK_SURFACE: u32 = 0x11f5;
pub const NT_GDI_ENUM_FONTS: u32 = 0x11f6;
pub const NT_GDI_ENUM_OBJECTS: u32 = 0x11f7;
pub const NT_GDI_EUDC_LOAD_UNLOAD_LINK: u32 = 0x11f8;
pub const NT_GDI_EXT_FLOOD_FILL: u32 = 0x11f9;
pub const NT_GDI_FONTOBJ_C_GET_ALL_GLYPH_HANDLES: u32 = 0x11fa;
pub const NT_GDI_FONTOBJ_C_GET_GLYPHS: u32 = 0x11fb;
pub const NT_GDI_FONTOBJ_P_QUERY_GLYPH_ATTRS: u32 = 0x11fc;
pub const NT_GDI_FONTOBJ_PFDG: u32 = 0x11fd;
pub const NT_GDI_FONTOBJ_PIFI: u32 = 0x11fe;
pub const NT_GDI_FONTOBJ_PV_TRUE_TYPE_FONT_FILE: u32 = 0x11ff;
pub const NT_GDI_FONTOBJ_PXO_GET_XFORM: u32 = 0x1200;
pub const NT_GDI_FONTOBJ_V_GET_INFO: u32 = 0x1201;
pub const NT_GDI_FLATTEN_PATH: u32 = 0x1202;
pub const NT_GDI_FONT_IS_LINKED: u32 = 0x1203;
pub const NT_GDI_FORCE_UFIMAPPING: u32 = 0x1204;
pub const NT_GDI_FRAME_RGN: u32 = 0x1205;
pub const NT_GDI_FULLSCREEN_CONTROL: u32 = 0x1206;
pub const NT_GDI_GET_BOUNDS_RECT: u32 = 0x1207;
pub const NT_GDI_GET_COPPCOMPATIBLE_OPMINFORMATION: u32 = 0x1208;
pub const NT_GDI_GET_CERTIFICATE: u32 = 0x1209;
pub const NT_GDI_GET_CERTIFICATE_SIZE: u32 = 0x120a;
pub const NT_GDI_GET_CHAR_ABCWIDTHS_W: u32 = 0x120b;
pub const NT_GDI_GET_CHARACTER_PLACEMENT_W: u32 = 0x120c;
pub const NT_GDI_GET_COLOR_ADJUSTMENT: u32 = 0x120d;
pub const NT_GDI_GET_COLOR_SPACEFOR_BITMAP: u32 = 0x120e;
pub const NT_GDI_GET_DEVICE_CAPS: u32 = 0x120f;
pub const NT_GDI_GET_DEVICE_CAPS_ALL: u32 = 0x1210;
pub const NT_GDI_GET_DEVICE_GAMMA_RAMP: u32 = 0x1211;
pub const NT_GDI_GET_DEVICE_WIDTH: u32 = 0x1212;
pub const NT_GDI_GET_DHPDEV: u32 = 0x1213;
pub const NT_GDI_GET_ETM: u32 = 0x1214;
pub const NT_GDI_GET_EMB_UFI: u32 = 0x1215;
pub const NT_GDI_GET_EMBED_FONTS: u32 = 0x1216;
pub const NT_GDI_GET_EUDC_TIME_STAMP_EX: u32 = 0x1217;
pub const NT_GDI_GET_FONT_FILE_DATA: u32 = 0x1218;
pub const NT_GDI_GET_FONT_FILE_INFO: u32 = 0x1219;
pub const NT_GDI_GET_FONT_RESOURCE_INFO_INTERNAL_W: u32 = 0x121a;
pub const NT_GDI_GET_FONT_UNICODE_RANGES: u32 = 0x121b;
pub const NT_GDI_GET_GLYPH_INDICES_W: u32 = 0x121c;
pub const NT_GDI_GET_GLYPH_INDICES_WINTERNAL: u32 = 0x121d;
pub const NT_GDI_GET_GLYPH_OUTLINE: u32 = 0x121e;
pub const NT_GDI_GET_KERNING_PAIRS: u32 = 0x121f;
pub const NT_GDI_GET_LINKED_UFIS: u32 = 0x1220;
pub const NT_GDI_GET_MITER_LIMIT: u32 = 0x1221;
pub const NT_GDI_GET_MONITOR_ID: u32 = 0x1222;
pub const NT_GDI_GET_NUMBER_OF_PHYSICAL_MONITORS: u32 = 0x1223;
pub const NT_GDI_GET_OPMINFORMATION: u32 = 0x1224;
pub const NT_GDI_GET_OPMRANDOM_NUMBER: u32 = 0x1225;
pub const NT_GDI_GET_OBJECT_BITMAP_HANDLE: u32 = 0x1226;
pub const NT_GDI_GET_PATH: u32 = 0x1227;
pub const NT_GDI_GET_PER_BAND_INFO: u32 = 0x1228;
pub const NT_GDI_GET_PHYSICAL_MONITOR_DESCRIPTION: u32 = 0x1229;
pub const NT_GDI_GET_PHYSICAL_MONITORS: u32 = 0x122a;
pub const NT_GDI_GET_REALIZATION_INFO: u32 = 0x122b;
pub const NT_GDI_GET_SERVER_META_FILE_BITS: u32 = 0x122c;
pub const NT_GDI_GET_SPOOL_MESSAGE: u32 = 0x122d;
pub const NT_GDI_GET_STATS: u32 = 0x122e;
pub const NT_GDI_GET_STRING_BITMAP_W: u32 = 0x122f;
pub const NT_GDI_GET_SUGGESTED_OPMPROTECTED_OUTPUT_ARRAY_SIZE: u32 = 0x1230;
pub const NT_GDI_GET_TEXT_EXTENT_EX_W: u32 = 0x1231;
pub const NT_GDI_GET_UFI: u32 = 0x1232;
pub const NT_GDI_GET_UFIPATHNAME: u32 = 0x1233;
pub const NT_GDI_GRADIENT_FILL: u32 = 0x1234;
pub const NT_GDI_HLSURF_GET_INFORMATION: u32 = 0x1235;
pub const NT_GDI_HLSURF_SET_INFORMATION: u32 = 0x1236;
pub const NT_GDI_HT_GET8_BPPFORMAT_PALETTE: u32 = 0x1237;
pub const NT_GDI_HT_GET8_BPPMASK_PALETTE: u32 = 0x1238;
pub const NT_GDI_ICM_BRUSH_INFO: u32 = 0x1239;
pub const NT_GDI_INIT: u32 = 0x123a;
pub const NT_GDI_INIT_SPOOL: u32 = 0x123b;
pub const NT_GDI_MAKE_FONT_DIR: u32 = 0x123c;
pub const NT_GDI_MAKE_INFO_DC: u32 = 0x123d;
pub const NT_GDI_MAKE_OBJECT_UN_XFERABLE: u32 = 0x123e;
pub const NT_GDI_MAKE_OBJECT_XFERABLE: u32 = 0x123f;
pub const NT_GDI_MIRROR_WINDOW_ORG: u32 = 0x1240;
pub const NT_GDI_MONO_BITMAP: u32 = 0x1241;
pub const NT_GDI_MOVE_TO: u32 = 0x1242;
pub const NT_GDI_OFFSET_CLIP_RGN: u32 = 0x1243;
pub const NT_GDI_PATHOBJ_B_ENUM: u32 = 0x1244;
pub const NT_GDI_PATHOBJ_B_ENUM_CLIP_LINES: u32 = 0x1245;
pub const NT_GDI_PATHOBJ_V_ENUM_START: u32 = 0x1246;
pub const NT_GDI_PATHOBJ_V_ENUM_START_CLIP_LINES: u32 = 0x1247;
pub const NT_GDI_PATHOBJ_V_GET_BOUNDS: u32 = 0x1248;
pub const NT_GDI_PATH_TO_REGION: u32 = 0x1249;
pub const NT_GDI_PLG_BLT: u32 = 0x124a;
pub const NT_GDI_POLY_DRAW: u32 = 0x124b;
pub const NT_GDI_POLY_TEXT_OUT_W: u32 = 0x124c;
pub const NT_GDI_PT_IN_REGION: u32 = 0x124d;
pub const NT_GDI_PT_VISIBLE: u32 = 0x124e;
pub const NT_GDI_QUERY_FONTS: u32 = 0x124f;
pub const NT_GDI_REMOVE_FONT_RESOURCE_W: u32 = 0x1250;
pub const NT_GDI_REMOVE_MERGE_FONT: u32 = 0x1251;
pub const NT_GDI_RESET_DC: u32 = 0x1252;
pub const NT_GDI_RESIZE_PALETTE: u32 = 0x1253;
pub const NT_GDI_ROUND_RECT: u32 = 0x1254;
pub const NT_GDI_STROBJ_B_ENUM: u32 = 0x1255;
pub const NT_GDI_STROBJ_B_ENUM_POSITIONS_ONLY: u32 = 0x1256;
pub const NT_GDI_STROBJ_B_GET_ADVANCE_WIDTHS: u32 = 0x1257;
pub const NT_GDI_STROBJ_DW_GET_CODE_PAGE: u32 = 0x1258;
pub const NT_GDI_STROBJ_V_ENUM_START: u32 = 0x1259;
pub const NT_GDI_SCALE_VIEWPORT_EXT_EX: u32 = 0x125a;
pub const NT_GDI_SCALE_WINDOW_EXT_EX: u32 = 0x125b;
pub const NT_GDI_SELECT_BRUSH: u32 = 0x125c;
pub const NT_GDI_SELECT_CLIP_PATH: u32 = 0x125d;
pub const NT_GDI_SELECT_PEN: u32 = 0x125e;
pub const NT_GDI_SET_BITMAP_ATTRIBUTES: u32 = 0x125f;
pub const NT_GDI_SET_BRUSH_ATTRIBUTES: u32 = 0x1260;
pub const NT_GDI_SET_COLOR_ADJUSTMENT: u32 = 0x1261;
pub const NT_GDI_SET_COLOR_SPACE: u32 = 0x1262;
pub const NT_GDI_SET_DEVICE_GAMMA_RAMP: u32 = 0x1263;
pub const NT_GDI_SET_FONT_XFORM: u32 = 0x1264;
pub const NT_GDI_SET_ICM_MODE: u32 = 0x1265;
pub const NT_GDI_SET_LINKED_UFIS: u32 = 0x1266;
pub const NT_GDI_SET_MAGIC_COLORS: u32 = 0x1267;
pub const NT_GDI_SET_OPMSIGNING_KEY_AND_SEQUENCE_NUMBERS: u32 = 0x1268;
pub const NT_GDI_SET_PUMPDOBJ: u32 = 0x1269;
pub const NT_GDI_SET_PIXEL_FORMAT: u32 = 0x126a;
pub const NT_GDI_SET_RECT_RGN: u32 = 0x126b;
pub const NT_GDI_SET_SIZE_DEVICE: u32 = 0x126c;
pub const NT_GDI_SET_SYSTEM_PALETTE_USE: u32 = 0x126d;
pub const NT_GDI_SET_TEXT_JUSTIFICATION: u32 = 0x126e;
pub const NT_GDI_SFM_GET_NOTIFICATION_TOKENS: u32 = 0x126f;
pub const NT_GDI_START_DOC: u32 = 0x1270;
pub const NT_GDI_START_PAGE: u32 = 0x1271;
pub const NT_GDI_STROKE_AND_FILL_PATH: u32 = 0x1272;
pub const NT_GDI_STROKE_PATH: u32 = 0x1273;
pub const NT_GDI_SWAP_BUFFERS: u32 = 0x1274;
pub const NT_GDI_TRANSPARENT_BLT: u32 = 0x1275;
pub const NT_GDI_UMPDENG_FREE_USER_MEM: u32 = 0x1276;
pub const NT_GDI_UNLOAD_PRINTER_DRIVER: u32 = 0x1277;
pub const NT_GDI_UNMAP_MEM_FONT: u32 = 0x1278;
pub const NT_GDI_UPDATE_COLORS: u32 = 0x1279;
pub const NT_GDI_UPDATE_TRANSFORM: u32 = 0x127a;
pub const NT_GDI_WIDEN_PATH: u32 = 0x127b;
pub const NT_GDI_XFORMOBJ_B_APPLY_XFORM: u32 = 0x127c;
pub const NT_GDI_XFORMOBJ_I_GET_XFORM: u32 = 0x127d;
pub const NT_GDI_XLATEOBJ_C_GET_PALETTE: u32 = 0x127e;
pub const NT_GDI_XLATEOBJ_H_GET_COLOR_TRANSFORM: u32 = 0x127f;
pub const NT_GDI_XLATEOBJ_I_XLATE: u32 = 0x1280;

// ----------------------------------------------------------------------------
// USER Services - Window Management
// ----------------------------------------------------------------------------

pub const NT_USER_GET_THREAD_STATE: u32 = 0x1000;
pub const NT_USER_PEEK_MESSAGE: u32 = 0x1001;
pub const NT_USER_CALL_ONE_PARAM: u32 = 0x1002;
pub const NT_USER_GET_KEY_STATE: u32 = 0x1003;
pub const NT_USER_INVALIDATE_RECT: u32 = 0x1004;
pub const NT_USER_CALL_NO_PARAM: u32 = 0x1005;
pub const NT_USER_GET_MESSAGE: u32 = 0x1006;
pub const NT_USER_MESSAGE_CALL: u32 = 0x1007;
pub const NT_USER_GET_DC: u32 = 0x100a;
pub const NT_USER_WAIT_MESSAGE: u32 = 0x100c;
pub const NT_USER_TRANSLATE_MESSAGE: u32 = 0x100d;
pub const NT_USER_GET_PROP: u32 = 0x100e;
pub const NT_USER_POST_MESSAGE: u32 = 0x100f;
pub const NT_USER_QUERY_WINDOW: u32 = 0x1010;
pub const NT_USER_TRANSLATE_ACCELERATOR: u32 = 0x1011;
pub const NT_USER_REDRAW_WINDOW: u32 = 0x1013;
pub const NT_USER_WINDOW_FROM_POINT: u32 = 0x1014;
pub const NT_USER_CALL_MSG_FILTER: u32 = 0x1015;
pub const NT_USER_VALIDATE_TIMER_CALLBACK: u32 = 0x1016;
pub const NT_USER_BEGIN_PAINT: u32 = 0x1017;
pub const NT_USER_SET_TIMER: u32 = 0x1018;
pub const NT_USER_END_PAINT: u32 = 0x1019;
pub const NT_USER_SET_CURSOR: u32 = 0x101a;
pub const NT_USER_KILL_TIMER: u32 = 0x101b;
pub const NT_USER_BUILD_HWND_LIST: u32 = 0x101c;
pub const NT_USER_SELECT_PALETTE: u32 = 0x101d;
pub const NT_USER_CALL_NEXT_HOOK_EX: u32 = 0x101e;
pub const NT_USER_HIDE_CARET: u32 = 0x101f;
pub const NT_USER_CALL_HWND_LOCK: u32 = 0x1021;
pub const NT_USER_GET_PROCESS_WINDOW_STATION: u32 = 0x1022;
pub const NT_USER_SET_WINDOW_POS: u32 = 0x1024;
pub const NT_USER_SHOW_CARET: u32 = 0x1025;
pub const NT_USER_END_DEFER_WINDOW_POS_EX: u32 = 0x1026;
pub const NT_USER_CALL_HWND_PARAM_LOCK: u32 = 0x1027;
pub const NT_USER_VK_KEY_SCAN_EX: u32 = 0x1028;
pub const NT_USER_CALL_TWO_PARAM: u32 = 0x102a;
pub const NT_USER_COPY_ACCELERATOR_TABLE: u32 = 0x102c;
pub const NT_USER_NOTIFY_WIN_EVENT: u32 = 0x102d;
pub const NT_USER_IS_CLIPBOARD_FORMAT_AVAILABLE: u32 = 0x102f;
pub const NT_USER_SET_SCROLL_INFO: u32 = 0x1030;
pub const NT_USER_CREATE_CARET: u32 = 0x1032;
pub const NT_USER_DISPATCH_MESSAGE: u32 = 0x1036;
pub const NT_USER_REGISTER_WINDOW_MESSAGE: u32 = 0x1037;
pub const NT_USER_GET_FOREGROUND_WINDOW: u32 = 0x103c;
pub const NT_USER_SHOW_SCROLL_BAR: u32 = 0x103d;
pub const NT_USER_FIND_EXISTING_CURSOR_ICON: u32 = 0x103e;
pub const NT_USER_SYSTEM_PARAMETERS_INFO: u32 = 0x1042;
pub const NT_USER_GET_ASYNC_KEY_STATE: u32 = 0x1044;
pub const NT_USER_GET_CPD: u32 = 0x1045;
pub const NT_USER_REMOVE_PROP: u32 = 0x1046;
pub const NT_USER_SET_CAPTURE: u32 = 0x1049;
pub const NT_USER_ENUM_DISPLAY_MONITORS: u32 = 0x104a;
pub const NT_USER_SET_PROP: u32 = 0x104c;
pub const NT_USER_SBGET_PARMS: u32 = 0x104e;
pub const NT_USER_GET_ICON_INFO: u32 = 0x104f;
pub const NT_USER_EXCLUDE_UPDATE_RGN: u32 = 0x1050;
pub const NT_USER_SET_FOCUS: u32 = 0x1051;
pub const NT_USER_DEFER_WINDOW_POS: u32 = 0x1053;
pub const NT_USER_GET_UPDATE_RECT: u32 = 0x1054;
pub const NT_USER_GET_CLIPBOARD_SEQUENCE_NUMBER: u32 = 0x1056;
pub const NT_USER_SHOW_WINDOW: u32 = 0x1058;
pub const NT_USER_GET_KEYBOARD_LAYOUT_LIST: u32 = 0x1059;
pub const NT_USER_MAP_VIRTUAL_KEY_EX: u32 = 0x105b;
pub const NT_USER_SET_WINDOW_LONG: u32 = 0x105c;
pub const NT_USER_MOVE_WINDOW: u32 = 0x105e;
pub const NT_USER_POST_THREAD_MESSAGE: u32 = 0x105f;
pub const NT_USER_DRAW_ICON_EX: u32 = 0x1060;
pub const NT_USER_GET_SYSTEM_MENU: u32 = 0x1061;
pub const NT_USER_INTERNAL_GET_WINDOW_TEXT: u32 = 0x1063;
pub const NT_USER_GET_WINDOW_DC: u32 = 0x1064;
pub const NT_USER_SCROLL_DC: u32 = 0x106b;
pub const NT_USER_GET_OBJECT_INFORMATION: u32 = 0x106c;
pub const NT_USER_FIND_WINDOW_EX: u32 = 0x106e;
pub const NT_USER_UNHOOK_WINDOWS_HOOK_EX: u32 = 0x1070;
pub const NT_USER_CREATE_WINDOW_EX: u32 = 0x1076;
pub const NT_USER_SET_PARENT: u32 = 0x1077;
pub const NT_USER_GET_KEYBOARD_STATE: u32 = 0x1078;
pub const NT_USER_TO_UNICODE_EX: u32 = 0x1079;
pub const NT_USER_GET_CONTROL_BRUSH: u32 = 0x107a;
pub const NT_USER_GET_CLASS_NAME: u32 = 0x107b;
pub const NT_USER_DEF_SET_TEXT: u32 = 0x107f;
pub const NT_USER_SEND_INPUT: u32 = 0x1082;
pub const NT_USER_GET_THREAD_DESKTOP: u32 = 0x1083;
pub const NT_USER_GET_UPDATE_RGN: u32 = 0x1086;
pub const NT_USER_GET_ICON_SIZE: u32 = 0x1088;
pub const NT_USER_FILL_WINDOW: u32 = 0x1089;
pub const NT_USER_SET_WINDOWS_HOOK_EX: u32 = 0x108c;
pub const NT_USER_NOTIFY_PROCESS_CREATE: u32 = 0x108d;
pub const NT_USER_GET_TITLE_BAR_INFO: u32 = 0x108f;
pub const NT_USER_SET_THREAD_DESKTOP: u32 = 0x1091;
pub const NT_USER_GET_DCEX: u32 = 0x1092;
pub const NT_USER_GET_SCROLL_BAR_INFO: u32 = 0x1093;
pub const NT_USER_SET_WINDOW_FNID: u32 = 0x1095;
pub const NT_USER_CALC_MENU_BAR: u32 = 0x1097;
pub const NT_USER_THUNKED_MENU_ITEM_INFO: u32 = 0x1098;
pub const NT_USER_DESTROY_CURSOR: u32 = 0x109c;
pub const NT_USER_DESTROY_WINDOW: u32 = 0x109d;
pub const NT_USER_CALL_HWND_PARAM: u32 = 0x109e;
pub const NT_USER_OPEN_WINDOW_STATION: u32 = 0x10a0;
pub const NT_USER_SET_CURSOR_ICON_DATA: u32 = 0x10a4;
pub const NT_USER_CLOSE_DESKTOP: u32 = 0x10a6;
pub const NT_USER_OPEN_DESKTOP: u32 = 0x10a7;
pub const NT_USER_SET_PROCESS_WINDOW_STATION: u32 = 0x10a8;
pub const NT_USER_GET_ATOM_NAME: u32 = 0x10a9;
pub const NT_USER_BUILD_NAME_LIST: u32 = 0x10ae;
pub const NT_USER_REGISTER_CLASS_EX_WOW: u32 = 0x10b0;
pub const NT_USER_GET_ANCESTOR: u32 = 0x10b2;
pub const NT_USER_CLOSE_WINDOW_STATION: u32 = 0x10b5;
pub const NT_USER_GET_DOUBLE_CLICK_TIME: u32 = 0x10b6;
pub const NT_USER_ENABLE_SCROLL_BAR: u32 = 0x10b7;
pub const NT_USER_GET_CLASS_INFO_EX: u32 = 0x10b9;
pub const NT_USER_UNREGISTER_CLASS: u32 = 0x10bb;
pub const NT_USER_DELETE_MENU: u32 = 0x10bc;
pub const NT_USER_SCROLL_WINDOW_EX: u32 = 0x10be;
pub const NT_USER_SET_CLASS_LONG: u32 = 0x10c0;
pub const NT_USER_GET_MENU_BAR_INFO: u32 = 0x10c1;
pub const NT_USER_INVALIDATE_RGN: u32 = 0x10c8;
pub const NT_USER_GET_CLIPBOARD_OWNER: u32 = 0x10c9;
pub const NT_USER_SET_WINDOW_RGN: u32 = 0x10ca;
pub const NT_USER_BIT_BLT_SYS_BMP: u32 = 0x10cb;
pub const NT_USER_VALIDATE_RECT: u32 = 0x10cd;
pub const NT_USER_CLOSE_CLIPBOARD: u32 = 0x10ce;
pub const NT_USER_OPEN_CLIPBOARD: u32 = 0x10cf;
pub const NT_USER_SET_CLIPBOARD_DATA: u32 = 0x10d1;
pub const NT_USER_ENABLE_MENU_ITEM: u32 = 0x10d2;
pub const NT_USER_ALTER_WINDOW_STYLE: u32 = 0x10d3;
pub const NT_USER_GET_WINDOW_PLACEMENT: u32 = 0x10d5;
pub const NT_USER_GET_OPEN_CLIPBOARD_WINDOW: u32 = 0x10d8;
pub const NT_USER_SET_THREAD_STATE: u32 = 0x10d9;
pub const NT_USER_TRACK_MOUSE_EVENT: u32 = 0x10db;
pub const NT_USER_DESTROY_MENU: u32 = 0x10dd;
pub const NT_USER_CONSOLE_CONTROL: u32 = 0x10df;
pub const NT_USER_SET_ACTIVE_WINDOW: u32 = 0x10e0;
pub const NT_USER_SET_INFORMATION_THREAD: u32 = 0x10e1;
pub const NT_USER_SET_WINDOW_PLACEMENT: u32 = 0x10e2;
pub const NT_USER_GET_CONTROL_COLOR: u32 = 0x10e3;
pub const NT_USER_SET_WINDOW_WORD: u32 = 0x10e8;
pub const NT_USER_GET_CLIPBOARD_FORMAT_NAME: u32 = 0x10e9;
pub const NT_USER_REAL_INTERNAL_GET_MESSAGE: u32 = 0x10ea;
pub const NT_USER_CREATE_LOCAL_MEM_HANDLE: u32 = 0x10eb;
pub const NT_USER_ATTACH_THREAD_INPUT: u32 = 0x10ec;
pub const NT_USER_PAINT_MENU_BAR: u32 = 0x10ee;
pub const NT_USER_SET_KEYBOARD_STATE: u32 = 0x10ef;
pub const NT_USER_CREATE_ACCELERATOR_TABLE: u32 = 0x10f1;
pub const NT_USER_GET_CURSOR_FRAME_INFO: u32 = 0x10f2;
pub const NT_USER_GET_ALT_TAB_INFO: u32 = 0x10f3;
pub const NT_USER_GET_CARET_BLINK_TIME: u32 = 0x10f4;
pub const NT_USER_PROCESS_CONNECT: u32 = 0x10f6;
pub const NT_USER_ENUM_DISPLAY_DEVICES: u32 = 0x10f7;
pub const NT_USER_EMPTY_CLIPBOARD: u32 = 0x10f8;
pub const NT_USER_GET_CLIPBOARD_DATA: u32 = 0x10f9;
pub const NT_USER_REMOVE_MENU: u32 = 0x10fa;
pub const NT_USER_CONVERT_MEM_HANDLE: u32 = 0x10fd;
pub const NT_USER_DESTROY_ACCELERATOR_TABLE: u32 = 0x10fe;
pub const NT_USER_GET_GUITHREAD_INFO: u32 = 0x10ff;
pub const NT_USER_SET_WINDOWS_HOOK_AW: u32 = 0x1101;
pub const NT_USER_SET_MENU_DEFAULT_ITEM: u32 = 0x1102;
pub const NT_USER_CHECK_MENU_ITEM: u32 = 0x1103;
pub const NT_USER_SET_WIN_EVENT_HOOK: u32 = 0x1104;
pub const NT_USER_UNHOOK_WIN_EVENT: u32 = 0x1105;
pub const NT_USER_LOCK_WINDOW_UPDATE: u32 = 0x1106;
pub const NT_USER_SET_SYSTEM_MENU: u32 = 0x1107;
pub const NT_USER_THUNKED_MENU_INFO: u32 = 0x1108;
pub const NT_USER_CALL_HWND: u32 = 0x110c;
pub const NT_USER_DDE_INITIALIZE: u32 = 0x110d;
pub const NT_USER_MODIFY_USER_STARTUP_INFO_FLAGS: u32 = 0x110e;
pub const NT_USER_COUNT_CLIPBOARD_FORMATS: u32 = 0x110f;
pub const NT_USER_ENUM_DISPLAY_SETTINGS: u32 = 0x1114;
pub const NT_USER_PAINT_DESKTOP: u32 = 0x1115;
pub const NT_USER_CHANGE_CLIPBOARD_CHAIN: u32 = 0x1119;
pub const NT_USER_SET_CLIPBOARD_VIEWER: u32 = 0x111a;
pub const NT_USER_SHOW_WINDOW_ASYNC: u32 = 0x111b;
pub const NT_USER_ACTIVATE_KEYBOARD_LAYOUT: u32 = 0x111e;
pub const NT_USER_ADD_CLIPBOARD_FORMAT_LISTENER: u32 = 0x1281;
pub const NT_USER_ASSOCIATE_INPUT_CONTEXT: u32 = 0x1282;
pub const NT_USER_BLOCK_INPUT: u32 = 0x1283;
pub const NT_USER_BUILD_HIMC_LIST: u32 = 0x1284;
pub const NT_USER_BUILD_PROP_LIST: u32 = 0x1285;
pub const NT_USER_CALCULATE_POPUP_WINDOW_POSITION: u32 = 0x1286;
pub const NT_USER_CALL_HWND_OPT: u32 = 0x1287;
pub const NT_USER_CHANGE_DISPLAY_SETTINGS: u32 = 0x1288;
pub const NT_USER_CHANGE_WINDOW_MESSAGE_FILTER_EX: u32 = 0x1289;
pub const NT_USER_CHECK_ACCESS_FOR_INTEGRITY_LEVEL: u32 = 0x128a;
pub const NT_USER_CHECK_DESKTOP_BY_THREAD_ID: u32 = 0x128b;
pub const NT_USER_CHECK_WINDOW_THREAD_DESKTOP: u32 = 0x128c;
pub const NT_USER_CHILD_WINDOW_FROM_POINT_EX: u32 = 0x128d;
pub const NT_USER_CLIP_CURSOR: u32 = 0x128e;
pub const NT_USER_CREATE_DESKTOP_EX: u32 = 0x128f;
pub const NT_USER_CREATE_INPUT_CONTEXT: u32 = 0x1290;
pub const NT_USER_CREATE_WINDOW_STATION: u32 = 0x1291;
pub const NT_USER_CTX_DISPLAY_IOCTL: u32 = 0x1292;
pub const NT_USER_DESTROY_INPUT_CONTEXT: u32 = 0x1293;
pub const NT_USER_DISABLE_THREAD_IME: u32 = 0x1294;
pub const NT_USER_DISPLAY_CONFIG_GET_DEVICE_INFO: u32 = 0x1295;
pub const NT_USER_DISPLAY_CONFIG_SET_DEVICE_INFO: u32 = 0x1296;
pub const NT_USER_DO_SOUND_CONNECT: u32 = 0x1297;
pub const NT_USER_DO_SOUND_DISCONNECT: u32 = 0x1298;
pub const NT_USER_DRAG_DETECT: u32 = 0x1299;
pub const NT_USER_DRAG_OBJECT: u32 = 0x129a;
pub const NT_USER_DRAW_ANIMATED_RECTS: u32 = 0x129b;
pub const NT_USER_DRAW_CAPTION: u32 = 0x129c;
pub const NT_USER_DRAW_CAPTION_TEMP: u32 = 0x129d;
pub const NT_USER_DRAW_MENU_BAR_TEMP: u32 = 0x129e;
pub const NT_USER_DWM_START_REDIRECTION: u32 = 0x129f;
pub const NT_USER_DWM_STOP_REDIRECTION: u32 = 0x12a0;
pub const NT_USER_END_MENU: u32 = 0x12a1;
pub const NT_USER_END_TOUCH_OPERATION: u32 = 0x12a2;
pub const NT_USER_EVENT: u32 = 0x12a3;
pub const NT_USER_FLASH_WINDOW_EX: u32 = 0x12a4;
pub const NT_USER_FROST_CRASHED_WINDOW: u32 = 0x12a5;
pub const NT_USER_GET_APP_IME_LEVEL: u32 = 0x12a6;
pub const NT_USER_GET_CARET_POS: u32 = 0x12a7;
pub const NT_USER_GET_CLIP_CURSOR: u32 = 0x12a8;
pub const NT_USER_GET_CLIPBOARD_VIEWER: u32 = 0x12a9;
pub const NT_USER_GET_COMBO_BOX_INFO: u32 = 0x12aa;
pub const NT_USER_GET_CURSOR_INFO: u32 = 0x12ab;
pub const NT_USER_GET_DISPLAY_CONFIG_BUFFER_SIZES: u32 = 0x12ac;
pub const NT_USER_GET_GESTURE_CONFIG: u32 = 0x12ad;
pub const NT_USER_GET_GESTURE_EXT_ARGS: u32 = 0x12ae;
pub const NT_USER_GET_GESTURE_INFO: u32 = 0x12af;
pub const NT_USER_GET_GUI_RESOURCES: u32 = 0x12b0;
pub const NT_USER_GET_IME_HOT_KEY: u32 = 0x12b1;
pub const NT_USER_GET_IME_INFO_EX: u32 = 0x12b2;
pub const NT_USER_GET_INPUT_LOCALE_INFO: u32 = 0x12b3;
pub const NT_USER_GET_INTERNAL_WINDOW_POS: u32 = 0x12b4;
pub const NT_USER_GET_KEY_NAME_TEXT: u32 = 0x12b5;
pub const NT_USER_GET_KEYBOARD_LAYOUT_NAME: u32 = 0x12b6;
pub const NT_USER_GET_LAYERED_WINDOW_ATTRIBUTES: u32 = 0x12b7;
pub const NT_USER_GET_LIST_BOX_INFO: u32 = 0x12b8;
pub const NT_USER_GET_MENU_INDEX: u32 = 0x12b9;
pub const NT_USER_GET_MENU_ITEM_RECT: u32 = 0x12ba;
pub const NT_USER_GET_MOUSE_MOVE_POINTS_EX: u32 = 0x12bb;
pub const NT_USER_GET_PRIORITY_CLIPBOARD_FORMAT: u32 = 0x12bc;
pub const NT_USER_GET_RAW_INPUT_BUFFER: u32 = 0x12bd;
pub const NT_USER_GET_RAW_INPUT_DATA: u32 = 0x12be;
pub const NT_USER_GET_RAW_INPUT_DEVICE_INFO: u32 = 0x12bf;
pub const NT_USER_GET_RAW_INPUT_DEVICE_LIST: u32 = 0x12c0;
pub const NT_USER_GET_REGISTERED_RAW_INPUT_DEVICES: u32 = 0x12c1;
pub const NT_USER_GET_TOP_LEVEL_WINDOW: u32 = 0x12c2;
pub const NT_USER_GET_TOUCH_INPUT_INFO: u32 = 0x12c3;
pub const NT_USER_GET_UPDATED_CLIPBOARD_FORMATS: u32 = 0x12c4;
pub const NT_USER_GET_WOWCLASS: u32 = 0x12c5;
pub const NT_USER_GET_WINDOW_COMPOSITION_ATTRIBUTE: u32 = 0x12c6;
pub const NT_USER_GET_WINDOW_COMPOSITION_INFO: u32 = 0x12c7;
pub const NT_USER_GET_WINDOW_DISPLAY_AFFINITY: u32 = 0x12c8;
pub const NT_USER_GET_WINDOW_MINIMIZE_RECT: u32 = 0x12c9;
pub const NT_USER_GET_WINDOW_RGN_EX: u32 = 0x12ca;
pub const NT_USER_GHOST_WINDOW_FROM_HUNG_WINDOW: u32 = 0x12cb;
pub const NT_USER_HARD_ERROR_CONTROL: u32 = 0x12cc;
pub const NT_USER_HILITE_MENU_ITEM: u32 = 0x12cd;
pub const NT_USER_HUNG_WINDOW_FROM_GHOST_WINDOW: u32 = 0x12ce;
pub const NT_USER_HWND_QUERY_REDIRECTION_INFO: u32 = 0x12cf;
pub const NT_USER_HWND_SET_REDIRECTION_INFO: u32 = 0x12d0;
pub const NT_USER_IMPERSONATE_DDE_CLIENT_WINDOW: u32 = 0x12d1;
pub const NT_USER_INIT_TASK: u32 = 0x12d2;
pub const NT_USER_INITIALIZE: u32 = 0x12d3;
pub const NT_USER_INITIALIZE_CLIENT_PFN_ARRAYS: u32 = 0x12d4;
pub const NT_USER_INJECT_GESTURE: u32 = 0x12d5;
pub const NT_USER_INTERNAL_GET_WINDOW_ICON: u32 = 0x12d6;
pub const NT_USER_IS_TOP_LEVEL_WINDOW: u32 = 0x12d7;
pub const NT_USER_IS_TOUCH_WINDOW: u32 = 0x12d8;
pub const NT_USER_LOAD_KEYBOARD_LAYOUT_EX: u32 = 0x12d9;
pub const NT_USER_LOCK_WINDOW_STATION: u32 = 0x12da;
pub const NT_USER_LOCK_WORK_STATION: u32 = 0x12db;
pub const NT_USER_LOGICAL_TO_PHYSICAL_POINT: u32 = 0x12dc;
pub const NT_USER_MNDRAG_LEAVE: u32 = 0x12dd;
pub const NT_USER_MNDRAG_OVER: u32 = 0x12de;
pub const NT_USER_MAG_CONTROL: u32 = 0x12df;
pub const NT_USER_MAG_GET_CONTEXT_INFORMATION: u32 = 0x12e0;
pub const NT_USER_MAG_SET_CONTEXT_INFORMATION: u32 = 0x12e1;
pub const NT_USER_MANAGE_GESTURE_HANDLER_WINDOW: u32 = 0x12e2;
pub const NT_USER_MENU_ITEM_FROM_POINT: u32 = 0x12e3;
pub const NT_USER_MIN_MAXIMIZE: u32 = 0x12e4;
pub const NT_USER_MODIFY_WINDOW_TOUCH_CAPABILITY: u32 = 0x12e5;
pub const NT_USER_NOTIFY_IMESTATUS: u32 = 0x12e6;
pub const NT_USER_OPEN_INPUT_DESKTOP: u32 = 0x12e7;
pub const NT_USER_OPEN_THREAD_DESKTOP: u32 = 0x12e8;
pub const NT_USER_PAINT_MONITOR: u32 = 0x12e9;
pub const NT_USER_PHYSICAL_TO_LOGICAL_POINT: u32 = 0x12ea;
pub const NT_USER_PRINT_WINDOW: u32 = 0x12eb;
pub const NT_USER_QUERY_DISPLAY_CONFIG: u32 = 0x12ec;
pub const NT_USER_QUERY_INFORMATION_THREAD: u32 = 0x12ed;
pub const NT_USER_QUERY_INPUT_CONTEXT: u32 = 0x12ee;
pub const NT_USER_QUERY_SEND_MESSAGE: u32 = 0x12ef;
pub const NT_USER_REAL_CHILD_WINDOW_FROM_POINT: u32 = 0x12f0;
pub const NT_USER_REAL_WAIT_MESSAGE_EX: u32 = 0x12f1;
pub const NT_USER_REGISTER_ERROR_REPORTING_DIALOG: u32 = 0x12f2;
pub const NT_USER_REGISTER_HOT_KEY: u32 = 0x12f3;
pub const NT_USER_REGISTER_RAW_INPUT_DEVICES: u32 = 0x12f4;
pub const NT_USER_REGISTER_SERVICES_PROCESS: u32 = 0x12f5;
pub const NT_USER_REGISTER_SESSION_PORT: u32 = 0x12f6;
pub const NT_USER_REGISTER_TASKLIST: u32 = 0x12f7;
pub const NT_USER_REGISTER_USER_API_HOOK: u32 = 0x12f8;
pub const NT_USER_REMOTE_CONNECT: u32 = 0x12f9;
pub const NT_USER_REMOTE_REDRAW_RECTANGLE: u32 = 0x12fa;
pub const NT_USER_REMOTE_REDRAW_SCREEN: u32 = 0x12fb;
pub const NT_USER_REMOTE_STOP_SCREEN_UPDATES: u32 = 0x12fc;
pub const NT_USER_REMOVE_CLIPBOARD_FORMAT_LISTENER: u32 = 0x12fd;
pub const NT_USER_RESOLVE_DESKTOP_FOR_WOW: u32 = 0x12fe;
pub const NT_USER_SEND_TOUCH_INPUT: u32 = 0x12ff;
pub const NT_USER_SET_APP_IME_LEVEL: u32 = 0x1300;
pub const NT_USER_SET_CHILD_WINDOW_NO_ACTIVATE: u32 = 0x1301;
pub const NT_USER_SET_CLASS_WORD: u32 = 0x1302;
pub const NT_USER_SET_CURSOR_CONTENTS: u32 = 0x1303;
pub const NT_USER_SET_DISPLAY_CONFIG: u32 = 0x1304;
pub const NT_USER_SET_GESTURE_CONFIG: u32 = 0x1305;
pub const NT_USER_SET_IME_HOT_KEY: u32 = 0x1306;
pub const NT_USER_SET_IME_INFO_EX: u32 = 0x1307;
pub const NT_USER_SET_IME_OWNER_WINDOW: u32 = 0x1308;
pub const NT_USER_SET_INTERNAL_WINDOW_POS: u32 = 0x1309;
pub const NT_USER_SET_LAYERED_WINDOW_ATTRIBUTES: u32 = 0x130a;
pub const NT_USER_SET_MENU: u32 = 0x130b;
pub const NT_USER_SET_MENU_CONTEXT_HELP_ID: u32 = 0x130c;
pub const NT_USER_SET_MENU_FLAG_RTO_L: u32 = 0x130d;
pub const NT_USER_SET_MIRROR_RENDERING: u32 = 0x130e;
pub const NT_USER_SET_OBJECT_INFORMATION: u32 = 0x130f;
pub const NT_USER_SET_PROCESS_DPIAWARE: u32 = 0x1310;
pub const NT_USER_SET_SHELL_WINDOW_EX: u32 = 0x1311;
pub const NT_USER_SET_SYS_COLORS: u32 = 0x1312;
pub const NT_USER_SET_SYSTEM_CURSOR: u32 = 0x1313;
pub const NT_USER_SET_SYSTEM_TIMER: u32 = 0x1314;
pub const NT_USER_SET_THREAD_LAYOUT_HANDLES: u32 = 0x1315;
pub const NT_USER_SET_WINDOW_COMPOSITION_ATTRIBUTE: u32 = 0x1316;
pub const NT_USER_SET_WINDOW_DISPLAY_AFFINITY: u32 = 0x1317;
pub const NT_USER_SET_WINDOW_RGN_EX: u32 = 0x1318;
pub const NT_USER_SET_WINDOW_STATION_USER: u32 = 0x1319;
pub const NT_USER_SFM_DESTROY_LOGICAL_SURFACE_BINDING: u32 = 0x131a;
pub const NT_USER_SFM_DX_BIND_SWAP_CHAIN: u32 = 0x131b;
pub const NT_USER_SFM_DX_GET_SWAP_CHAIN_STATS: u32 = 0x131c;
pub const NT_USER_SFM_DX_OPEN_SWAP_CHAIN: u32 = 0x131d;
pub const NT_USER_SFM_DX_QUERY_SWAP_CHAIN_BINDING_STATUS: u32 = 0x131e;
pub const NT_USER_SFM_DX_RELEASE_SWAP_CHAIN: u32 = 0x131f;
pub const NT_USER_SFM_DX_REPORT_PENDING_BINDINGS_TO_DWM: u32 = 0x1320;
pub const NT_USER_SFM_DX_SET_SWAP_CHAIN_BINDING_STATUS: u32 = 0x1321;
pub const NT_USER_SFM_DX_SET_SWAP_CHAIN_STATS: u32 = 0x1322;
pub const NT_USER_SFM_GET_LOGICAL_SURFACE_BINDING: u32 = 0x1323;
pub const NT_USER_SHOW_SYSTEM_CURSOR: u32 = 0x1324;
pub const NT_USER_SOUND_SENTRY: u32 = 0x1325;
pub const NT_USER_SWITCH_DESKTOP: u32 = 0x1326;
pub const NT_USER_TEST_FOR_INTERACTIVE_USER: u32 = 0x1327;
pub const NT_USER_TRACK_POPUP_MENU_EX: u32 = 0x1328;
pub const NT_USER_UNLOAD_KEYBOARD_LAYOUT: u32 = 0x1329;
pub const NT_USER_UNLOCK_WINDOW_STATION: u32 = 0x132a;
pub const NT_USER_UNREGISTER_HOT_KEY: u32 = 0x132b;
pub const NT_USER_UNREGISTER_SESSION_PORT: u32 = 0x132c;
pub const NT_USER_UNREGISTER_USER_API_HOOK: u32 = 0x132d;
pub const NT_USER_UPDATE_INPUT_CONTEXT: u32 = 0x132e;
pub const NT_USER_UPDATE_INSTANCE: u32 = 0x132f;
pub const NT_USER_UPDATE_LAYERED_WINDOW: u32 = 0x1330;
pub const NT_USER_UPDATE_PER_USER_SYSTEM_PARAMETERS: u32 = 0x1331;
pub const NT_USER_UPDATE_WINDOW_TRANSFORM: u32 = 0x1332;
pub const NT_USER_USER_HANDLE_GRANT_ACCESS: u32 = 0x1333;
pub const NT_USER_VALIDATE_HANDLE_SECURE: u32 = 0x1334;
pub const NT_USER_WAIT_FOR_INPUT_IDLE: u32 = 0x1335;
pub const NT_USER_WAIT_FOR_MSG_AND_EVENT: u32 = 0x1336;
pub const NT_USER_WINDOW_FROM_PHYSICAL_POINT: u32 = 0x1337;
pub const NT_USER_YIELD_TASK: u32 = 0x1338;
pub const NT_USER_SET_CLASS_LONG_PTR: u32 = 0x1339;
pub const NT_USER_SET_WINDOW_LONG_PTR: u32 = 0x133a;

// =====================================================================
// Shadow SSDT Dispatch
// =====================================================================

/// Dispatch a shadow system service call
///
/// Bits 12-15 select the service table; win32k uses table one.
/// Only complete numbers in `0x1000..0x2000` select this table;
/// upper bits must not alias a valid service.
pub fn dispatch_shadow_service(
    service_number: u32,
    syscall_frame: &crate::arch::common::syscall_frame::SyscallFrame,
) -> u64 {
    if !is_win32k_syscall(service_number) {
        return 0xC000001Cu32 as u64; // STATUS_INVALID_SYSTEM_SERVICE
    }
    let service_index = service_number - 0x1000;

    match get_shadow_service(service_index) {
        Some(handler) => unsafe {
            let fn_ptr: extern "C" fn(&crate::arch::common::syscall_frame::SyscallFrame) -> u64 =
                core::mem::transmute(handler);
            fn_ptr(syscall_frame)
        },
        None if (service_index as usize) < nt61_abi::syscalls::WIN32K_SYSCALLS.len() => {
            0xC0000002u32 as u64 // Known Win7 service, STATUS_NOT_IMPLEMENTED
        }
        None => 0xC000001Cu32 as u64, // STATUS_INVALID_SYSTEM_SERVICE
    }
}

/// Check if a syscall number is a shadow (win32k) syscall
#[inline(always)]
pub fn is_win32k_syscall(syscall_num: u32) -> bool {
    (0x1000..0x2000).contains(&syscall_num)
}

/// Get the service index from a win32k syscall number
#[inline(always)]
pub fn get_win32k_service_index(syscall_num: u32) -> u32 {
    syscall_num & 0x0FFF
}

// =====================================================================
// Smoke Test
// =====================================================================

/// Run Shadow SSDT smoke test
pub fn smoke_test() -> bool {
    // // kprintln!("  [SHADOW SSDT SMOKE] running Shadow SSDT smoke test...");

    let table_ptr = unsafe { (*core::ptr::addr_of!(SHADOW_SERVICE_TABLE)).as_ptr() };
    if table_ptr.is_null() {
        // // kprintln!("  [SHADOW SSDT SMOKE FAIL] ShadowServiceTable is null");
        return false;
    }

    let arg_ptr = unsafe { (*core::ptr::addr_of!(SHADOW_ARGUMENT_TABLE)).as_ptr() };
    if arg_ptr.is_null() {
        // // kprintln!("  [SHADOW SSDT SMOKE FAIL] ShadowArgumentTable is null");
        return false;
    }

    // // kprintln!("  [SHADOW SSDT SMOKE] ShadowServiceTable: {:p}", table_ptr);
    // // kprintln!("  [SHADOW SSDT SMOKE] ShadowArgumentTable: {:p}", arg_ptr);
    // // kprintln!("  [SHADOW SSDT SMOKE] Max shadow services: {}", SHADOW_MAX_SERVICES);
    // // kprintln!("  [SHADOW SSDT SMOKE] is_win32k_syscall(0x1234) = {}", is_win32k_syscall(0x1234));
    // // kprintln!("  [SHADOW SSDT SMOKE OK]");
    true
}
