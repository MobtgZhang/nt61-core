//! Execute the production argument capture with a bounded user-memory fixture.
use std::cell::RefCell;
thread_local! {
    static STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static READS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}
mod mm {
    pub mod user_copy {
        pub fn read_user_object<T: Copy>(address: u64) -> Result<T, i32> {
            crate::READS.with(|reads| reads.borrow_mut().push(address));
            if address < 0x1000 || address & 7 != 0 {
                return Err(0xC0000005u32 as i32);
            }
            crate::STACK.with(|stack| {
                let value = stack
                    .borrow()
                    .get(((address - 0x1000) / 8) as usize)
                    .copied()
                    .ok_or(0xC0000005u32 as i32)?;
                assert_eq!(core::mem::size_of::<T>(), core::mem::size_of::<usize>());
                // SAFETY: the test-only producer is instantiated with usize.
                Ok(unsafe { core::ptr::read((&value as *const usize).cast::<T>()) })
            })
        }
    }
}
#[path = "../src/ke/ssdt_arguments.rs"]
mod arguments;
use arguments::*;

fn fixture(registers: usize) -> SyscallArguments {
    STACK.with(|stack| *stack.borrow_mut() = (registers..17).map(|index| 0xA0 + index).collect());
    READS.with(|reads| reads.borrow_mut().clear());
    let mut args = SyscallArguments {
        integer_register_count: registers,
        stack_pointer: 0x1000,
        ..SyscallArguments::default()
    };
    for index in 0..registers {
        args.register_args[index] = 0xA0 + index;
    }
    args
}

#[test]
fn all_four_integer_abis_preserve_seventeen_arguments() {
    for registers in [4, SEVEN_REGISTER_ARCHITECTURES, INTEGER_REGISTER_ARGUMENTS] {
        let captured = capture_register_arguments(&fixture(registers), 17).unwrap();
        for index in 0..17 {
            assert_eq!(
                read_argument(&captured, index),
                Ok(0xA0 + index),
                "regs={registers}, index={index}"
            );
        }
    }
}

#[test]
fn short_calls_do_not_probe_unused_stack_slots() {
    let args = SyscallArguments {
        register_args: [0xAA; 8],
        integer_register_count: 4,
        stack_pointer: u64::MAX as usize,
    };
    let captured = capture_register_arguments(&args, 1).unwrap();
    assert_eq!(read_argument(&captured, 0), Ok(0xAA));
    READS.with(|reads| assert!(reads.borrow().is_empty()));
}

#[test]
fn unreadable_or_wrapped_trailing_slots_fail_before_handler_runs() {
    let mut args = fixture(4);
    args.stack_pointer = 0;
    assert_eq!(
        capture_register_arguments(&args, 5).unwrap_err(),
        0xC0000005
    );
    args.stack_pointer = usize::MAX - 3;
    assert_eq!(read_argument(&args, 5), Err(0xC0000005));
    args.integer_register_count = 9;
    assert_eq!(
        capture_register_arguments(&args, 5).unwrap_err(),
        0xC000000D
    );
}

#[test]
fn create_thread_trailing_slots_begin_at_argument_seven() {
    let captured = capture_register_arguments(&fixture(7), 9).unwrap();
    assert_eq!(read_argument(&captured, 7), Ok(0xA7));
    assert_eq!(read_argument(&captured, 8), Ok(0xA8));
}

#[test]
fn calls_beyond_the_shipped_seventeen_argument_limit_are_rejected() {
    assert_eq!(
        capture_register_arguments(&fixture(8), 18).unwrap_err(),
        0xC000000D
    );
}

mod libs {
    pub mod ntdll {
        pub mod status {
            pub const STATUS_INVALID_SYSTEM_SERVICE: i32 = 0xC000001Cu32 as i32;
            pub const STATUS_INVALID_PARAMETER: i32 = 0xC000000Du32 as i32;
            pub const STATUS_ACCESS_VIOLATION: i32 = 0xC0000005u32 as i32;
        }
    }
}
mod ke {
    pub mod ssdt {
        pub fn get_shadow_argument_size(number: u32) -> u8 {
            match number {
                0x1000 => 17 * 8,
                0x1001 => 8,
                0x1002 => 18 * 8,
                _ => 0,
            }
        }
    }
}
#[path = "../../arch/src/arch/common/syscall_frame.rs"]
mod syscall_frame;

#[test]
fn shadow_capture_preserves_seventeen_arguments_for_every_integer_abi() {
    for registers in [4, SEVEN_REGISTER_ARCHITECTURES, INTEGER_REGISTER_ARGUMENTS] {
        let args = fixture(registers);
        let values: Vec<u64> = args.register_args[..registers]
            .iter()
            .map(|value| *value as u64)
            .collect();
        let captured =
            syscall_frame::SyscallFrame::capture_shadow(0x1000, &values, 0x1000).unwrap();
        assert_eq!(
            captured.args,
            core::array::from_fn(|index| 0xA0 + index as u64)
        );
    }
}

#[test]
fn shadow_capture_rejects_bad_abi_windows_and_ignores_unused_stack() {
    let captured =
        syscall_frame::SyscallFrame::capture_shadow(0x1001, &[0xAA; 4], u64::MAX).unwrap();
    assert_eq!(captured.args[0], 0xAA);
    assert!(syscall_frame::SyscallFrame::capture_shadow(0x2000, &[], 0).is_err());
    assert!(syscall_frame::SyscallFrame::capture_shadow(0x1002, &[], 0).is_err());
    assert!(syscall_frame::SyscallFrame::capture_shadow(0x1000, &[0; 4], u64::MAX).is_err());
    assert_eq!(
        syscall_frame::SyscallFrame::from_args([1; 17]).args,
        [1; 17]
    );
}
