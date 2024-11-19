//! A shell pallet built with [`frame`].
//!
//! Implementation of CHIP-8 Emulation on Rust Substrate's FRAME blockchain runtime framework.
//! This is a reference implementation from then original implementation [`CHIP-8 Emulation on EVM`](https://www.piapark.me/chip-8-emulation-on-evm/)

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{alloc, Decode, Encode};
use frame::prelude::*;
use polkadot_sdk::polkadot_sdk_frame as frame;
// Re-export all pallet parts, this is needed to properly import the pallet into the runtime.
pub use pallet::*;

/// Font set containing the sprite data for hexadecimal digits (0-9 and A-F).
/// Each character sprite consists of 5 bytes, representing an 8x5 monochrome grid.
const FONTSET: [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
    0x20, 0x60, 0x20, 0x20, 0x70, // 1
    0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
    0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
    0x90, 0x90, 0xF0, 0x10, 0x10, // 4
    0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
    0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
    0xF0, 0x10, 0x20, 0x40, 0x40, // 7
    0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
    0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
    0xF0, 0x90, 0xF0, 0x90, 0x90, // A
    0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
    0xF0, 0x80, 0x80, 0x80, 0xF0, // C
    0xE0, 0x90, 0x90, 0x90, 0xE0, // D
    0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
    0xF0, 0x80, 0xF0, 0x80, 0x80, // F
];

/// starting address for programs
const START_ADDR: u16 = 0x200;

#[frame_support::pallet(dev_mode)]
pub mod pallet {
    use super::*;

    // ------------------------------* core components *--------------------------------- //

    /// The main emulator struct
    #[derive(Clone, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
    pub struct Emulator {
        /// 16 bit program counter
        pub pc: u16,
        /// 4kb (4096 bits) sized virtual RAM
        pub ram: [u8; 4096],
        /// Display: 64 x 32 pixels as bitfields (8 rows of 256 bits each)
        pub display: [u8; 256],
        /// Sixteen 8-bit general-purpose registers (V0 to VF).
        pub virtual_registers: [u8; 16],
        /// A 16-bit register for memory access (I register).
        pub i_register: u16,
        /// stack pointer
        pub sp: u16,
        /// stack for subroutine calls
        pub stack: [u8; 16],
        /// Keyboard state as a 16-bit bitfield.
        pub keys: u16,
        /// Delay timer
        pub dt: u8,
        /// Sound timer
        pub st: u8,
        /// Size of the loaded program
        pub program_size: primitive_types::U256,
    }

    impl Default for Emulator {
        fn default() -> Self {
            Emulator {
                ram: [0; 4096],
                display: [0; 256],
                virtual_registers: [0; 16],
                stack: [0; 16],
                ..Default::default()
            }
        }
    }

    #[pallet::storage]
    pub type emulator<T> = StorageValue<_, Emulator, ValueQuery>;
    #[pallet::config(with_default)]
    pub trait Config: frame_system::Config {
        #[pallet::no_default_bounds]
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
    }

    /// genesis state, load tye fontset in the emulator RAM
    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        _phantom: PhantomData<T>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            emulator::<T>::mutate(|emu| {
                emu.pc = START_ADDR;
                emu.ram[0..=80].copy_from_slice(&FONTSET);
            })
        }
    }
    #[pallet::pallet]
    pub struct Pallet<T>(_);

    // ---------------------------* hooks function *--------------------------- //
    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_finalize(_n:BlockNumber){
            Self::execute();
        }
    }

    // ---------------------------* tasks function *--------------------------- //

    // ---------------------------* emulation function *--------------------------- //
    impl<T: Config> Pallet<T>{

        /// pop value from stack
        pub fn pop() -> u16 {
            let emu = emulator::<T>::get();
            ensure!(emu.sp < 1, Error::<T>::StackUndeflow);
            emulator::<T>::mutate(|emu|{
                emu.sp -= 1;
            });
            emu.stack[emu.sp]
        }

        /// push value from stack
        pub fn push(val:u16){
            let emu = emulator::<T>::get();
            // it should be less that stack size (16)
            ensure!(emu.sp < 16, Error::<T>::StackOverflow);
            emulator::<T>::mutate(|emu|{
                emu.stack.push(val);
                emu.sp += 1;
            });
        }

        /// CPU processing loop
        /// This function is called once per tick of the CPU.
        /// Fetch the next instruction, decode and execute it.
        pub fn tick(){
            // Fetch
            let op_code = Self::fetch();
            // Decode & execute
            Self::execute(op_code);

            Self::tick_timers();
        }

        pub fn tick_timers(){
            let emu = emulator::<T>::get();
            if emu.dt > 0 {
                emu.dt -= 1;
            }

            if emu.st > 0 {
                if emu.st == 1 {
                    // BEEP
                }
                emu.st -= 1;
            }
        }

        /// fetch the next instruction
        pub fn fetch() -> u16 {
            let emu = emulator::<T>::get();
            // if its less than RAM SiZE
            ensure!(emu.pc + 1 < 4096, Error::<T>::MemoryOutOfBounds);
            let higher_byte = emu.ram[emu.pc];
            let lower_byte = emu.ram[emu.pc + 1];
            // form the full opcode
            // example
            // higher_byte = 0xA2 = 1010 0010
            // after shift = 1010 0010 0000 0000 (0xA200)
            //
            // lower_byte  = 0xF0 = 0000 0000 1111 0000
            //
            // Result      = 1010 0010 1111 0000 (0xA2F0)
            let op_code = higher_byte << 8 | lower_byte;
            emu.pc += 2;
            emulator::<T>::set(emu);
            op_code
        }

        pub fn execute(op:u16){
            todo!()
        }
    }

    // ---------------------------* helper functions *--------------------------- //
    #[pallet::call]
    impl<T: Config> Pallet<T>{

        pub fn reset_emulator(origin:OriginFor<T>) -> DispatchResult {
            emulator::<T>::set(Default::default());
            // reset the program counter and ram for display
            emulator::<T>::mutate(|emu|{
                emu.pc = START_ADDR;
                emu.ram[0..=80].copy_from_slice(&FONTSET);
            });
            Ok(())
        }

        // ---------------------------* emulation function *--------------------------- //
        pub fn run(origin:OriginFor<T>) -> DispatchResult {
            let emu = emulator::<T>::get();

            ensure!(emu.program_size < 1.into(), Error::<T>::ProgramSizeZero);

            let end = emu.program_size.as_u128();
            for instr in 0..end {
                ensure!(emu.pc > 4096,Error::<T>::MemoryOutOfBounds);
                let op_code = Self::fetch();
                Self::execute(op_code);
            }
            Ok(())
        }
        pub fn load(origin: OriginFor<T>, data: Vec<u8>) -> DispatchResult {
            let start = START_ADDR;
            let end = start + data.len();
            ensure!(end < 4096, Error::<T>::ProgramSizeTooLarge);
            emulator::<T>::mutate(|emu|{
                emu.program_size = U256::from(data.len());
                emu.ram.as_slice.copy_from_slice(data.as_slice());
            });
            Ok(())
        }
        // ---------------------------* keyboard functions *--------------------------- //
        /// Handle keypress event
        /// Index of the key (0-15)
        /// pressed Whether the key is pressed (true) or released (false)
        pub fn keypress(origin: OriginFor<T>, index: u8, pressed: bool) -> DispatchResult {
            emulator::<T>::mutate(|emu|{
                if pressed {
                    // Set bit to 1 for pressed key
                    emu.keys |= 1u16 << idx;

                    // Example:
                    // index = 4
                    // 1 << 4 = 0000 0000 0001 0000
                    // keys |= makes that bit 1
                } else {
                    // Clear bit to 0 for released key
                    emu.keys &= !(1u16 << idx);

                    // Example:
                    // index = 4
                    // !(1 << 4) = 1111 1111 1110 1111
                    // keys &= clears that bit to 0
                }
            });
            Ok(())
        }
        // ---------------------------**********************--------------------------- //

        pub fn get_get_display(origin: OriginFor<T>) -> DispatchResult {
            let emu = emulator::<T>::get();

            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("screen")),
                value: Box::new(emu.display)
            });
            Ok(())
        }
        pub fn get_program_counter(origin: OriginFor<T>) -> DispatchResult {
            let emu = emulator::<T>::get();
            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("program_counter")),
                value: Box::new(emu.pc)
            });
            Ok(())
        }

        pub fn get_keyboard_keys(origin: OriginFor<T>) -> DispatchResult {
            let emu = emulator::<T>::get();
            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("keyboard_keys")),
                value: Box::new(emu.keys)
            });
            Ok(())
        }

        pub fn get_ram_value_at(origin: OriginFor<T>,index: u8) -> DispatchResult {
            let emu = emulator::<T>::get();

            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("ram value")),
                value: Box::new(emu.ram[index])
            });
            Ok(())
        }

        pub fn get_vregister(origin: OriginFor<T>,index: u8) -> DispatchResult {
            let emu = emulator::<T>::get();

            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("ram value")),
                value: Box::new(emu.virtual_registers[index])
            });
            Ok(())
        }

        pub fn get_iregister(origin:OriginFor<T>) -> DispatchResult {
            let emu = emulator::<T>::get();
            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("i_register")),
                value: Box::new(emu.i_register)
            });
            Ok(())
        }

        pub fn get_delay_timer(origin:OriginFor<T>) -> DispatchResult {
            let emu = emulator::<T>::get();
            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("delay timer")),
                value: Box::new(emu.dt)
            });
            Ok(())
        }

        pub fn get_sound_timer(origin:OriginFor<T>) -> DispatchResult {
            let emu = emulator::<T>::get();
            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("sound timer")),
                value: Box::new(emu.st)
            });
            Ok(())
        }

        pub fn get_stack_pointer(origin:OriginFor<T>) -> DispatchResult {
            let emu = emulator::<T>::get();
            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("stack pointer")),
                value: Box::new(emu.sp)
            });
            Ok(())
        }

        pub fn get_stack_value(origin:OriginFor<T>,index: u8) -> DispatchResult {
            let emu = emulator::<T>::get();
            self::deposit_event(Event::ReturnValue{
                name: Box::leak(Box::new("stack value")),
                value: Box::new(emu.stack[index])
            });
            Ok(())
        }

        pub fn set_vregister(origin:OriginFor<T>, index: u8, value: u8) -> DispatchResult {
            emulator::<T>::mutate(|emu|{
                emu.virtual_register[index] = value;
            });
            Ok(())
        }

        pub fn set_iregister(origin:OriginFor<T>,value: u16) -> DispatchResult {
            emulator::<T>::mutate(|emu|{
                emu.i_register = value;
            });
            Ok(())
        }

        pub fn set_ram_value_at(origin:OriginFor<T>,index: u8, value: u8) -> DispatchResult {
            emulator::<T>::mutate(|emu|{
                emu.ram[index] = value;
            });
            Ok(())
        }

        pub fn set_delay_timer(origin:OriginFor<T>,value: u8) -> DispatchResult {
            emulator::<T>::mutate(|emu|{
                emu.dt = value;
            });
            Ok(())
        }

        pub fn set_sound_timer(origin:OriginFor<T>,value: u8) -> DispatchResult{
            emulator::<T>::mutate(|emu|{
                emu.st = value;
            });
            Ok(())
        }

        pub fn set_stack_value(origin:OriginFor<T>,index: u8, value: u16) -> DispatchResult {
            emulator::<T>::mutate(|emu|{
                emu.stack[index] = value;
            });
            Ok(())
        }

        pub fn set_stack_pointer(origin:OriginFor<T>,value: u16) -> DispatchResult {
            emulator::<T>::mutate(|emu|{
                emu.sp = value;
            });
            Ok(())
        }

        pub fn set_screen_pixel(origin:OriginFor<T>,index: u8, value: bool) -> DispatchResult {
            let byte_index = index >> 8;      // Get byte position
            let bit_position = index & 255;    // Get bit position

            emulator::<T>::mutate(|emu|{
                if value {
                    self.screen[byte_index] |= 1 << bit_position;
                } else {
                    self.screen[byte_index] &= !(1 << bit_position);
                }
            });
            Ok(())
        }

        pub fn is_display_cleared(origin:OriginFor<T>,) -> DispatchResult {
            let emu = emulator::<T>::get();
            let is_cleared = {
                emu.screen[0] == 0 && emu.screen[1] == 0 && emu.screen[2] == 0 && emu.screen[3] == 0 && emu.screen[4] == 0
                    && emu.screen[5] == 0 && emu.screen[6] == 0 && emu.screen[7] == 0
            };
            self::deposit_event(
                Event::ReturnValue {
                    name: Box::leak(Box::new("display_cleared")),
                    value: Box::new(is_cleared)
                }
            );
            Ok(())
        }
    }
    #[pallet::error]
    pub enum Error<T> {
        /// when loaded program has zero size
        ProgramSizeZero,
        /// when loaded program is too big
        PrograSizeToolarge,
        /// when the program counter exceed RAM space
        MemoryOutOfBounds,
        /// stack underflow
        StackUndeflow,
        /// stack overflow
        StackOverflow
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ReturnValue {
            name: &'static str,
            value: Box<dyn Default>
        }
    }
}
