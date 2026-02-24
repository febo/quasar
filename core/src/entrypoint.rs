pub use solana_account_view::{
    AccountView, RuntimeAccount, MAX_PERMITTED_DATA_INCREASE, NOT_BORROWED,
};

#[macro_export]
macro_rules! dispatch {
    ($ptr:expr, $ix_data:expr, $disc_len:literal, {
        $([$($disc_byte:literal),+] => $handler:ident($accounts_ty:ty)),+ $(,)?
    }) => {{
        let __program_id: &[u8; 32] = unsafe {
            &*($ix_data.as_ptr().add($ix_data.len()) as *const [u8; 32])
        };
        let __accounts_start = unsafe { ($ptr as *mut u8).add(core::mem::size_of::<u64>()) };

        if $ix_data.len() < $disc_len {
            return Err(ProgramError::InvalidInstructionData);
        }
        let __disc: [u8; $disc_len] = unsafe {
            *($ix_data.as_ptr() as *const [u8; $disc_len])
        };
        match __disc {
            $(
                [$($disc_byte),+] => {
                    let mut __buf = core::mem::MaybeUninit::<
                        [AccountView; <$accounts_ty as AccountCount>::COUNT]
                    >::uninit();
                    let __remaining_ptr = unsafe {
                        <$accounts_ty>::parse_accounts(__accounts_start, &mut __buf)
                    };
                    let __accounts = unsafe { __buf.assume_init() };
                    $handler(Context {
                        program_id: __program_id,
                        accounts: &__accounts,
                        remaining_ptr: __remaining_ptr,
                        data: $ix_data,
                        accounts_boundary: unsafe { $ix_data.as_ptr().sub(core::mem::size_of::<u64>()) },
                    })
                }
            ),+
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }};
}

#[macro_export]
macro_rules! no_alloc {
    () => {
        pub mod allocator {
            extern crate alloc;
            pub struct NoAlloc;
            unsafe impl alloc::alloc::GlobalAlloc for NoAlloc {
                #[inline]
                unsafe fn alloc(&self, _: core::alloc::Layout) -> *mut u8 {
                    panic!("");
                }
                #[inline]
                unsafe fn dealloc(&self, _: *mut u8, _: core::alloc::Layout) {
                    // Can't dealloc if you never alloc ;)
                }
            }

            #[cfg(any(target_os = "solana", target_arch = "bpf"))]
            #[global_allocator]
            static A: NoAlloc = NoAlloc;
        }
    };
}

#[macro_export]
macro_rules! panic_handler {
    () => {
        #[cfg(any(target_os = "solana", target_arch = "bpf"))]
        fn panic(_info: &core::panic::PanicInfo<'_>) {
            solana_program_log::log("PANIC");
        }
    };
}

/// The bump allocator used as the default Rust heap when running programs.
#[cfg(feature = "alloc")]
#[macro_export]
macro_rules! heap_alloc {
    () => {
        pub mod allocator {
            extern crate alloc;
            use crate::utils::hint::unlikely;

            /// Maximum heap length in bytes that a program can request.
            pub const MAX_HEAP_LENGTH: u32 = 256 * 1024; // 256 KiB
            /// Start address of the memory region used for program heap.
            pub const HEAP_START_ADDRESS: u64 = 0x300000000;

            /// The bump allocator used as the default Rust heap when running programs.
            #[derive(Clone, Debug)]
            pub struct BumpAllocator {
                start: usize,
                end: usize,
            }
            impl BumpAllocator {
                /// Creates the allocator tied to specific range of addresses.
                pub const unsafe fn new_unchecked(start: usize, len: usize) -> Self {
                    Self {
                        start,
                        end: start + len,
                    }
                }
            }

            #[allow(clippy::arithmetic_side_effects)]
            unsafe impl alloc::alloc::GlobalAlloc for BumpAllocator {
                /// Allocates memory as described by the given `layout` using a forward
                /// bump allocator.
                #[inline]
                unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
                    // Reads the current position of the heap pointer.
                    let pos_ptr = self.start as *mut usize;
                    let mut pos = *pos_ptr;

                    if unlikely(pos == 0) {
                        // First time, set starting position.
                        pos = self.start + size_of::<usize>();
                    }

                    // Determines the allocation address, adjusting the alignment for the
                    // type being allocated.
                    let allocation = (pos + layout.align() - 1) & !(layout.align() - 1);

                    if unlikely(layout.size() > MAX_HEAP_LENGTH as usize)
                        || unlikely(self.end < allocation + layout.size())
                    {
                        return core::ptr::null_mut();
                    }

                    // Updates the heap pointer.
                    *pos_ptr = allocation + layout.size();

                    allocation as *mut u8
                }

                /// Behaves like `alloc`, but also ensures that the contents are set to
                /// zero before being returned.
                #[inline]
                unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                    self.alloc(layout)
                }

                /// This method has no effect since the bump allocator does not free
                /// memory.
                #[inline]
                unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
            }

            #[cfg(any(target_os = "solana", target_arch = "bpf"))]
            #[global_allocator]
            static A: BumpAllocator = unsafe {
                BumpAllocator::new_unchecked(
                    HEAP_START_ADDRESS as usize,
                    // Use the maximum heap length allowed. Programs can request heap sizes up
                    // to this value using the `ComputeBudget`.
                    MAX_HEAP_LENGTH as usize,
                )
            };

            /// A default allocator for when the program is compiled on a target different
            /// than `"solana"`.
            #[cfg(not(any(target_os = "solana", target_arch = "bpf")))]
            mod __private_alloc {
                extern crate std as __std;
            }
        }
    };
}
