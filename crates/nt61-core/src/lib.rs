//! NT6.1 kernel core implementation.
#![no_std]
#![allow(
    unused_unsafe,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::field_reassign_with_default,
    clippy::if_same_then_else,
    clippy::empty_line_after_doc_comments,
    clippy::wrong_self_convention,
    clippy::unnecessary_cast,
    clippy::manual_checked_ops,
    clippy::should_implement_trait,
    clippy::fn_to_numeric_cast,
    clippy::type_complexity,
    clippy::declare_interior_mutable_const,
    clippy::manual_strip,
    clippy::collapsible_match,
    clippy::while_let_loop,
    clippy::module_inception,
    clippy::explicit_counter_loop,
    clippy::duplicated_attributes,
    clippy::same_item_push,
    clippy::only_used_in_recursion,
    clippy::assertions_on_constants,
    clippy::iter_out_of_bounds,
    clippy::manual_flatten,
    clippy::len_without_is_empty,
    clippy::unnecessary_unwrap,
    clippy::large_enum_variant,
    clippy::box_collection,
    clippy::no_effect,
    clippy::redundant_locals,
    clippy::inherent_to_string,
    clippy::chunks_exact_to_as_chunks,
    clippy::not_unsafe_ptr_arg_deref,
    clippy::missing_safety_doc,
    clippy::doc_lazy_continuation,
    clippy::result_unit_err,
)]

extern crate alloc;
extern crate core;

pub mod arch;
pub mod crypto;
pub mod fs;
pub mod hal;
pub mod io;
pub mod ke;
pub mod loader;
pub mod lpc;
pub mod mm;
pub mod ob;
pub mod ps;
pub mod rtl;
pub mod se;

pub mod boot_types;
pub mod drivers;
pub mod kernel_main;
pub mod netstack;
pub mod libs;
pub mod servers;
pub mod registry;
pub mod userspace;
pub mod kernel_main_safe;
pub mod lib_atomic_helpers;
pub mod sync_helpers;

pub const KERNEL_VERSION: &str = "6.1.7601";
pub const KERNEL_BUILD: &str = "7601.win7_gdr";
