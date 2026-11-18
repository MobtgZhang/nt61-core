use alloc::{format, string::String};

// English (en-US) text for Control Panel pages. Keep user-facing labels here
// so a later language pack can replace this catalog without changing navigation.
pub const SYSTEM_AND_SECURITY: &str = "System and Security";
pub const NETWORK_AND_INTERNET: &str = "Network and Internet";
pub const HARDWARE_AND_SOUND: &str = "Hardware and Sound";
pub const PROGRAMS: &str = "Programs";
pub const USER_ACCOUNTS_AND_FAMILY_SAFETY: &str = "User Accounts and Family Safety";
pub const APPEARANCE_AND_PERSONALIZATION: &str = "Appearance and Personalization";
pub const CLOCK_LANGUAGE_AND_REGION: &str = "Clock, Language, and Region";
pub const EASE_OF_ACCESS: &str = "Ease of Access";
pub const REVIEW_YOUR_COMPUTER_S_STATUS: &str = "Review your computer's status";
pub const BACK_UP_YOUR_COMPUTER: &str = "Back up your computer";
pub const FIND_AND_FIX_PROBLEMS: &str = "Find and fix problems";
pub const VIEW_NETWORK_STATUS_AND_TASKS: &str = "View network status and tasks";
pub const CHOOSE_HOMEGROUP_AND_SHARING_OPTIONS: &str = "Choose homegroup and sharing options";
pub const VIEW_DEVICES_AND_PRINTERS: &str = "View devices and printers";
pub const ADD_A_DEVICE: &str = "Add a device";
pub const CONNECT_TO_A_PROJECTOR: &str = "Connect to a projector";
pub const ADJUST_COMMONLY_USED_MOBILITY_SETTINGS: &str = "Adjust commonly used mobility settings";
pub const UNINSTALL_A_PROGRAM: &str = "Uninstall a program";
pub const ADD_OR_REMOVE_USER_ACCOUNTS: &str = "Add or remove user accounts";
pub const SET_UP_PARENTAL_CONTROLS_FOR_ANY_USER: &str = "Set up parental controls for any user";
pub const CHANGE_THE_THEME: &str = "Change the theme";
pub const CHANGE_DESKTOP_BACKGROUND: &str = "Change desktop background";
pub const ADJUST_SCREEN_RESOLUTION: &str = "Adjust screen resolution";
pub const CHANGE_KEYBOARDS_OR_OTHER_INPUT_METHODS: &str = "Change keyboards or other input methods";
pub const CHANGE_DISPLAY_LANGUAGE: &str = "Change display language";
pub const LET_WINDOWS_SUGGEST_SETTINGS: &str = "Let Windows suggest settings";
pub const OPTIMIZE_VISUAL_DISPLAY: &str = "Optimize visual display";
pub const SYSTEM: &str = "System";
pub const SCREEN_RESOLUTION: &str = "Screen Resolution";
pub const DISPLAY: &str = "Display";
pub const PERSONALIZATION: &str = "Personalization";
pub const USER_ACCOUNTS: &str = "User Accounts";
pub const SOUND: &str = "Sound";
pub const DEVICE_MANAGER: &str = "Device Manager";
pub const NETWORK_AND_SHARING_CENTER: &str = "Network and Sharing Center";
pub const DEFAULT_PROGRAMS: &str = "Default Programs";
pub const EASE_OF_ACCESS_CENTER: &str = "Ease of Access Center";
pub const DATE_AND_TIME: &str = "Date and Time";
pub const ADMINISTRATIVE_TOOLS: &str = "Administrative Tools";
pub const CONTROL_PANEL: &str = "Control Panel";
pub const ALL_CONTROL_PANEL_ITEMS: &str = "All Control Panel Items";
pub const SYSTEM_PROPERTIES: &str = "System Properties";
pub const DISPLAY_ADAPTER_PROPERTIES: &str = "Display Adapter Properties";
pub const CHOOSING_DISPLAY_SETTINGS: &str = "Choosing Display Settings";
pub const SEARCH_CONTROL_PANEL: &str = "Search Control Panel";
pub const VIEW_BY: &str = "View by:";
pub const CATEGORY: &str = "Category ▾";
pub const LARGE_ICONS: &str = "Large icons ▾";
pub const SMALL_ICONS: &str = "Small icons ▾";
pub const CATEGORY_2: &str = "Category";
pub const LARGE_ICONS_2: &str = "Large icons";
pub const SMALL_ICONS_2: &str = "Small icons";
pub const SEARCH_RESULTS: &str = "Search Results";
pub const NO_ITEMS_MATCH_YOUR_SEARCH: &str = "No items match your search.";
pub const ADJUST_YOUR_COMPUTER_S_SETTINGS: &str = "Adjust your computer's settings";
pub const CONTROL_PANEL_HOME: &str = "Control Panel Home";
pub const DATE_AND_TIME_ARE_PROVIDED_BY_THE_SYSTEM_CLOCK: &str =
    "Date and time are provided by the system clock.";
pub const VIEW_SYSTEM_INFORMATION_AND_ADVANCED_SETTINGS: &str =
    "View system information and advanced settings";
pub const CHANGE_THE_RESOLUTION_OF_YOUR_DISPLAY: &str = "Change the resolution of your display";
pub const CHANGE_YOUR_THEME_BACKGROUND_AND_WINDOW_COLOR: &str =
    "Change your theme, background and window color";
pub const VIEW_HARDWARE_DEVICES_AND_DRIVERS: &str = "View hardware devices and drivers";
pub const CHANGE_ACCOUNT_TYPES_PASSWORDS_AND_PICTURES: &str =
    "Change account types, passwords and pictures";
pub const CONFIGURE_PLAYBACK_DEVICES_AND_SYSTEM_SOUNDS: &str =
    "Configure playback devices and system sounds";
pub const OPEN_THIS_CONTROL_PANEL_ITEM: &str = "Open this Control Panel item";
pub const COMPUTER_NAME: &str = "Computer Name";
pub const HARDWARE: &str = "Hardware";
pub const ADVANCED: &str = "Advanced";
pub const SYSTEM_PROTECTION: &str = "System Protection";
pub const REMOTE: &str = "Remote";
pub const WINDOWS_USES_THIS_INFORMATION_TO_IDENTIFY_YOUR_COMPUTER_ON_THE_NETWORK: &str =
    "Windows uses this information to identify your computer on the network.";
pub const FULL_COMPUTER_NAME_NT61: &str = "Full computer name:    NT61";
pub const WORKGROUP_NOT_JOINED: &str = "Workgroup:                 Not joined";
pub const COMPUTER_RENAMING_AND_DOMAIN_MEMBERSHIP_ARE_NOT_AVAILABLE: &str =
    "Computer renaming and domain membership are not available.";
pub const CHANGE: &str = "Change...";
pub const VIEW_AND_MANAGE_THE_DEVICES_AND_DRIVERS_INSTALLED_ON_THIS_COMPUTER: &str =
    "View and manage the devices and drivers installed on this computer.";
pub const PERFORMANCE: &str = "Performance";
pub const VISUAL_EFFECTS_PROCESSOR_SCHEDULING_MEMORY_USAGE_AND_VIRTUAL_MEMORY: &str =
    "Visual effects, processor scheduling, memory usage and virtual memory";
pub const USER_PROFILES: &str = "User Profiles";
pub const DESKTOP_SETTINGS_RELATED_TO_YOUR_SIGN_IN: &str =
    "Desktop settings related to your sign-in";
pub const STARTUP_AND_RECOVERY: &str = "Startup and Recovery";
pub const SYSTEM_STARTUP_SYSTEM_FAILURE_AND_DEBUGGING_INFORMATION: &str =
    "System startup, system failure and debugging information";
pub const SETTINGS: &str = "Settings...";
pub const THESE_SYSTEM_CONFIGURATION_SERVICES_ARE_NOT_AVAILABLE: &str =
    "These system configuration services are not available.";
pub const USE_SYSTEM_PROTECTION_TO_UNDO_UNWANTED_SYSTEM_CHANGES: &str =
    "Use system protection to undo unwanted system changes.";
pub const PROTECTION_SETTINGS: &str = "Protection Settings";
pub const AVAILABLE_DRIVES_PROTECTION: &str =
    "Available Drives                           Protection";
pub const LOCAL_DISK_C_OFF: &str = "Local Disk (C:)                             Off";
pub const SYSTEM_RESTORE_SNAPSHOTS_ARE_NOT_AVAILABLE: &str =
    "System Restore snapshots are not available.";
pub const SYSTEM_RESTORE: &str = "System Restore...";
pub const REMOTE_ASSISTANCE: &str = "Remote Assistance";
pub const ALLOW_REMOTE_ASSISTANCE_CONNECTIONS_TO_THIS_COMPUTER: &str =
    "□ Allow Remote Assistance connections to this computer";
pub const REMOTE_DESKTOP: &str = "Remote Desktop";
pub const DON_T_ALLOW_CONNECTIONS_TO_THIS_COMPUTER: &str =
    "◉ Don't allow connections to this computer";
pub const REMOTE_DESKTOP_SERVICES_ARE_NOT_AVAILABLE: &str =
    "Remote Desktop services are not available.";
pub const OK: &str = "OK";
pub const CANCEL: &str = "Cancel";
pub const APPLY: &str = "Apply";
pub const RETURN_TO_SCREEN_RESOLUTION: &str = "Return to Screen Resolution";
pub const ADAPTER_CURRENT_SYSTEM_DISPLAY_DRIVER: &str = "Adapter:    Current system display driver";
pub const MONITOR_GENERIC_NON_PNP_MONITOR: &str = "Monitor:    Generic Non-PnP Monitor";
pub const COLORS_TRUE_COLOR_32_BIT: &str = "Colors:      True Color (32 bit)";
pub const MODES_REPORTED_BY_THE_CURRENT_DISPLAY_DRIVER: &str =
    "Modes reported by the current display driver:";
pub const MAKE_IT_EASIER_TO_READ_WHAT_S_ON_YOUR_SCREEN: &str =
    "Make it easier to read what's on your screen";
pub const YOU_CAN_CHANGE_THE_SIZE_OF_TEXT_AND_OTHER_ITEMS_ON_YOUR_SCREEN: &str =
    "You can change the size of text and other items on your screen.";
pub const SMALLER_100_DEFAULT: &str = "◉ Smaller - 100% (default)";
pub const MEDIUM_125: &str = "○ Medium - 125%";
pub const LARGER_150: &str = "○ Larger - 150%";
pub const THE_DESKTOP_USES_96_DPI_OTHER_SCALING_LEVELS_ARE_NOT_AVAILABLE: &str =
    "The desktop uses 96 DPI. Other scaling levels are not available.";
pub const WHAT_DISPLAY_SETTINGS_SHOULD_I_CHOOSE: &str = "What display settings should I choose?";
pub const RESOLUTION_DETERMINES_THE_NUMBER_OF_PIXELS_ON_YOUR_SCREEN: &str =
    "Resolution determines the number of pixels on your screen.";
pub const CHOOSE_A_RESOLUTION_SUPPORTED_BY_THE_DISPLAY_DRIVER: &str =
    "Choose a resolution supported by the display driver.";
pub const CLICK_APPLY_TO_SWITCH_TO_THE_SELECTED_RESOLUTION: &str =
    "Click Apply to switch to the selected resolution.";
pub const CONFIRM_WITHIN_15_SECONDS_TO_KEEP_YOUR_CHANGES: &str =
    "Confirm within 15 seconds to keep your changes.";
pub const IF_THE_SCREEN_IS_UNREADABLE_WAIT_FOR_THE_SETTINGS_TO_REVERT: &str =
    "If the screen is unreadable, wait for the settings to revert.";
pub const CANCEL_DISCARDS_CHANGES_THAT_HAVE_NOT_BEEN_APPLIED: &str =
    "Cancel discards changes that have not been applied.";
pub const DETECTED_ONE_MONITOR: &str = "Detected one monitor.";
pub const NO_ADDITIONAL_MONITORS_WERE_DETECTED: &str = "No additional monitors were detected.";
pub const CHANGE_THE_APPEARANCE_OF_YOUR_DISPLAY: &str = "Change the appearance of your display";
pub const DETECT: &str = "Detect";
pub const IDENTIFY: &str = "Identify";
pub const DISPLAY_2: &str = "Display:";
pub const LABEL_1_GENERIC_NON_PNP_MONITOR: &str = "1. Generic Non-PnP Monitor ▾";
pub const RESOLUTION: &str = "Resolution:";
pub const ADVANCED_SETTINGS: &str = "Advanced settings";
pub const MAKE_TEXT_AND_OTHER_ITEMS_LARGER_OR_SMALLER: &str =
    "Make text and other items larger or smaller";
pub const DO_YOU_WANT_TO_KEEP_THESE_DISPLAY_SETTINGS: &str =
    "Do you want to keep these display settings?";
pub const KEEP_CHANGES: &str = "Keep changes";
pub const REVERT: &str = "Revert";
pub const REMOTE_SETTINGS: &str = "Remote settings";
pub const ADVANCED_SYSTEM_SETTINGS: &str = "Advanced system settings";
pub const SEE_ALSO: &str = "See also";
pub const ACTION_CENTER: &str = "Action Center";
pub const PERFORMANCE_INFORMATION_AND_TOOLS: &str = "Performance Information and Tools";
pub const VIEW_BASIC_INFORMATION_ABOUT_YOUR_COMPUTER: &str =
    "View basic information about your computer";
pub const WINDOWS_EDITION: &str = "Windows edition";
pub const COMPUTER_NAME_DOMAIN_AND_WORKGROUP_SETTINGS: &str =
    "Computer name, domain, and workgroup settings";
pub const WINDOWS_ACTIVATION: &str = "Windows activation";
pub const WINDOWS_7_SP1_COMPATIBLE_SYSTEM: &str = "Windows 7 SP1 compatible system";
pub const VERSION_6_1_7601_SERVICE_PACK_1: &str = "Version 6.1.7601 · Service Pack 1";
pub const RATING: &str = "Rating:";
pub const SYSTEM_RATING_IS_NOT_AVAILABLE: &str = "System rating is not available";
pub const PROCESSOR: &str = "Processor:";
pub const INSTALLED_MEMORY_RAM: &str = "Installed memory (RAM):";
pub const NOT_REPORTED: &str = "Not reported";
pub const SYSTEM_TYPE: &str = "System type:";
pub const LABEL_64_BIT_OPERATING_SYSTEM: &str = "64-bit Operating System";
pub const PEN_AND_TOUCH: &str = "Pen and Touch:";
pub const NO_PEN_OR_TOUCH_INPUT_IS_AVAILABLE_FOR_THIS_DISPLAY: &str =
    "No Pen or Touch Input is available for this Display";
pub const COMPUTER_NAME_2: &str = "Computer name:";
pub const FULL_COMPUTER_NAME: &str = "Full computer name:";
pub const WORKGROUP: &str = "Workgroup:";
pub const NOT_JOINED: &str = "Not joined";
pub const CHANGE_SETTINGS: &str = "Change settings";
pub const MICROSOFT_WINDOWS_ACTIVATION_DOES_NOT_APPLY_TO_NT61: &str =
    "Microsoft Windows activation does not apply to NT61.";

pub const WINDOWS_UPDATE: &str = "Windows Update";

// Parameterized messages live with the catalog so language packs can change
// word order as well as individual labels.
pub fn restore_failed(status: u32) -> String {
    format!("Could not restore display settings (0x{status:08X}).")
}
pub fn mode_rejected(status: u32) -> String {
    format!("The display driver rejected this mode (0x{status:08X}).")
}
pub fn save_failed(status: u32) -> String {
    format!("Could not save display settings (0x{status:08X}).")
}
pub fn detect_failed(status: u32) -> String {
    format!("Could not detect monitors (0x{status:08X}).")
}
pub fn reverting_in(seconds: u64) -> String {
    format!("Reverting to the previous settings in {seconds} seconds.")
}
pub fn processor(architecture: &str, count: Option<u32>) -> String {
    count.map_or_else(
        || architecture.into(),
        |n| format!("{architecture} · {n} logical processors"),
    )
}
pub fn memory_mb(bytes: u64) -> String {
    format!("{} MB", bytes / 1048576)
}
