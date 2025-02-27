//! Inspired by [ftdi-embedded-hal] this is an [embedded-hal] implementation
//! for the for the FTDI chips using the [libftd2xx] drivers.
//!
//! This enables development of embedded device drivers without the use of a
//! microcontroller.
//! The FTDI D2xx devices interface with your PC via USB, and provide a
//! multi-protocol synchronous serial engine to interface with most UART, SPI,
//! and I2C embedded devices.
//!
//! **Note:**
//! This is strictly a development tool.
//! The crate contains runtime borrow checks and explicit panics to adapt the
//! FTDI device into the [embedded-hal] traits.
//!
//! # Setup
//!
//! One-time device setup instructions can be found in the [libftd2xx crate].
//!
//! # Examples
//!
//! * [newAM/eeprom25aa02e48-rs]
//! * [newAM/bme280-rs]
//!
//! ## SPI
//!
//! ```no_run
//! use embedded_hal::prelude::*;
//! use ftd2xx_embedded_hal::Ft232hHal;
//!
//! let ftdi = Ft232hHal::new()?.init_default()?;
//! let mut spi = ftdi.spi()?;
//! # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
//! ```
//!
//! ## I2C
//!
//! ```no_run
//! use embedded_hal::prelude::*;
//! use ftd2xx_embedded_hal::Ft232hHal;
//!
//! let ftdi = Ft232hHal::new()?.init_default()?;
//! let mut i2c = ftdi.i2c()?;
//! # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
//! ```
//!
//! # GPIO
//!
//! ```no_run
//! use embedded_hal::prelude::*;
//! use ftd2xx_embedded_hal::Ft232hHal;
//!
//! let ftdi = Ft232hHal::new()?.init_default()?;
//! let mut gpio = ftdi.ad6();
//! # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
//! ```
//!
//! # Limitations
//!
//! * Limited trait support: SPI, I2C, Delay, and OutputPin traits are implemented.
//! * Limited device support: FT232H.
//!
//! [embedded-hal]: https://crates.io/crates/embedded-hal
//! [ftdi-embedded-hal]: https://github.com/geomatsi/ftdi-embedded-hal
//! [libftd2xx crate]: https://github.com/newAM/libftd2xx-rs/
//! [libftd2xx]: https://github.com/newAM/libftd2xx-rs
//! [newAM/eeprom25aa02e48-rs]: https://github.com/newAM/eeprom25aa02e48-rs/blob/main/examples/ftdi.rs
//! [newAM/bme280-rs]: https://github.com/newAM/bme280-rs/blob/main/examples/ftdi.rs
#![doc(html_root_url = "https://docs.rs/ftd2xx-embedded-hal/0.5.1")]
#![deny(unsafe_code, missing_docs)]

pub use embedded_hal;
pub use libftd2xx;

mod delay;
mod gpio;
mod i2c;
mod spi;

pub use delay::Delay;
pub use gpio::OutputPin;
pub use i2c::I2c;
pub use spi::Spi;

use libftd2xx::{DeviceTypeError, Ft232h, Ftdi, FtdiMpsse, MpsseSettings, TimeoutError};
use std::{cell::RefCell, convert::TryInto, sync::Mutex, time::Duration};

/// State tracker for each pin on the FTDI chip.
#[derive(Debug, Clone, Copy)]
enum PinUse {
    I2c,
    Spi,
    Output,
}

impl std::fmt::Display for PinUse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PinUse::I2c => write!(f, "I2C"),
            PinUse::Spi => write!(f, "SPI"),
            PinUse::Output => write!(f, "GPIO"),
        }
    }
}

#[derive(Debug)]
struct Ft232hInner {
    /// FTDI device.
    ft: Ft232h,
    /// GPIO direction.
    direction: u8,
    /// GPIO value.
    value: u8,
    /// Pin allocation.
    pins: [Option<PinUse>; 8],
}

impl Ft232hInner {
    /// Allocate a pin for a specific use.
    pub fn allocate_pin(&mut self, idx: u8, purpose: PinUse) {
        assert!(idx < 8, "Pin index {} is out of range 0 - 7", idx);

        if let Some(current) = self.pins[usize::from(idx)] {
            panic!(
                "Unable to allocate pin {} for {}, pin is already allocated for {}",
                idx, purpose, current
            );
        } else {
            self.pins[usize::from(idx)] = Some(purpose)
        }
    }
}

impl From<Ft232h> for Ft232hInner {
    fn from(ft: Ft232h) -> Self {
        Ft232hInner {
            ft,
            direction: 0xFB,
            value: 0x00,
            pins: [None; 8],
        }
    }
}

/// Type state for an initialized FTDI HAL.
///
/// More information about type states can be found in the [rust-embedded book].
///
/// [rust-embedded book]: https://docs.rust-embedded.org/book/static-guarantees/design-contracts.html
pub struct Initialized;

/// Type state for an uninitialized FTDI HAL.
///
/// More information about type states can be found in the [rust-embedded book].
///
/// [rust-embedded book]: https://docs.rust-embedded.org/book/static-guarantees/design-contracts.html
pub struct Uninitialized;

/// FT232H device.
#[derive(Debug)]
pub struct Ft232hHal<INITIALIZED> {
    #[allow(dead_code)]
    init: INITIALIZED,
    mtx: Mutex<RefCell<Ft232hInner>>,
}

impl Ft232hHal<Uninitialized> {
    /// Create a new FT232H structure.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ftd2xx_embedded_hal as hal;
    ///
    /// let ftdi = hal::Ft232hHal::new()?.init_default()?;
    /// # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
    /// ```
    pub fn new() -> Result<Ft232hHal<Uninitialized>, DeviceTypeError> {
        let ft: Ft232h = Ftdi::new()?.try_into()?;
        Ok(ft.into())
    }

    /// Create a new FT232H structure from a serial number.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ftd2xx_embedded_hal as hal;
    ///
    /// let ftdi = hal::Ft232hHal::with_serial_number("FT6ASGXH")?.init_default()?;
    /// # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_serial_number(sn: &str) -> Result<Ft232hHal<Uninitialized>, DeviceTypeError> {
        let ft: Ft232h = Ft232h::with_serial_number(sn)?;
        Ok(ft.into())
    }

    /// Open a `Ft4232h` device by its device description.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use libftd2xx::Ft4232h;
    ///
    /// Ft4232h::with_description("FT4232H-56Q MiniModule A")?;
    /// # Ok::<(), libftd2xx::DeviceTypeError>(())
    /// ```
    pub fn with_description(
        description: &str,
    ) -> Result<Ft232hHal<Uninitialized>, DeviceTypeError> {
        let ft: Ft232h = Ft232h::with_description(description)?;
        Ok(ft.into())
    }

    /// Initialize the FTDI MPSSE with sane defaults.
    ///
    /// Default values:
    ///
    /// * Reset the FTDI device.
    /// * 4k USB transfer size.
    /// * 1s USB read timeout.
    /// * 1s USB write timeout.
    /// * 16ms latency timer.
    /// * 100kHz clock frequency.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ftd2xx_embedded_hal as hal;
    /// use hal::{Ft232hHal, Initialized, Uninitialized};
    ///
    /// let ftdi: Ft232hHal<Uninitialized> = hal::Ft232hHal::new()?;
    /// let ftdi: Ft232hHal<Initialized> = ftdi.init_default()?;
    /// # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
    /// ```
    pub fn init_default(self) -> Result<Ft232hHal<Initialized>, TimeoutError> {
        const DEFAULT: MpsseSettings = MpsseSettings {
            reset: true,
            in_transfer_size: 4096,
            read_timeout: Duration::from_secs(1),
            write_timeout: Duration::from_secs(1),
            latency_timer: Duration::from_millis(16),
            mask: 0x00,
            clock_frequency: Some(100_000),
        };

        self.init(&DEFAULT)
    }

    /// Initialize the FTDI MPSSE with custom values.
    ///
    /// **Note:** The `mask` field of [`MpsseSettings`] is ignored for this function.
    ///
    /// **Note:** The clock frequency will be 2/3 of the specified value when in
    /// I2C mode.
    ///
    /// # Panics
    ///
    /// Panics if the `clock_frequency` field of [`MpsseSettings`] is `None`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ftd2xx_embedded_hal as hal;
    /// use hal::libftd2xx::MpsseSettings;
    /// use hal::{Ft232hHal, Initialized, Uninitialized};
    ///
    /// let ftdi: Ft232hHal<Uninitialized> = hal::Ft232hHal::new()?;
    /// let ftdi: Ft232hHal<Initialized> = ftdi.init(&MpsseSettings {
    ///     clock_frequency: Some(500_000),
    ///     ..MpsseSettings::default()
    /// })?;
    ///
    /// # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// [`MpsseSettings`]: libftd2xx::MpsseSettings
    pub fn init(
        self,
        mpsse_settings: &MpsseSettings,
    ) -> Result<Ft232hHal<Initialized>, TimeoutError> {
        {
            let lock = self.mtx.lock().expect("Failed to aquire FTDI mutex");
            let mut inner = lock.borrow_mut();
            let mut settings = *mpsse_settings;
            settings.mask = inner.direction;
            inner.ft.initialize_mpsse(&mpsse_settings)?;
        }

        Ok(Ft232hHal {
            init: Initialized,
            mtx: self.mtx,
        })
    }
}

impl From<Ft232h> for Ft232hHal<Uninitialized> {
    /// Create a new FT232H structure from a specific FT232H device.
    ///
    /// # Examples
    ///
    /// Selecting a device with a specific serial number.
    ///
    /// ```no_run
    /// use ftd2xx_embedded_hal as hal;
    /// use hal::libftd2xx::Ft232h;
    /// use hal::Ft232hHal;
    ///
    /// let ft = Ft232h::with_serial_number("FT59UO4C")?;
    /// let ftdi = Ft232hHal::from(ft).init_default()?;
    /// # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Selecting a device with a specific description.
    ///
    /// ```no_run
    /// use ftd2xx_embedded_hal as hal;
    /// use hal::libftd2xx::Ft232h;
    /// use hal::Ft232hHal;
    ///
    /// let ft = Ft232h::with_description("My device description")?;
    /// let ftdi = Ft232hHal::from(ft).init_default()?;
    /// # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
    /// ```
    fn from(ft: Ft232h) -> Self {
        Ft232hHal {
            init: Uninitialized,
            mtx: Mutex::new(RefCell::new(ft.into())),
        }
    }
}

impl Ft232hHal<Initialized> {
    /// Aquire the SPI peripheral for the FT232H.
    ///
    /// Pin assignments:
    /// * AD0 => SCK
    /// * AD1 => MOSI
    /// * AD2 => MISO
    ///
    /// # Panics
    ///
    /// Panics if pin 0, 1, or 2 are already in use.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ftd2xx_embedded_hal as hal;
    ///
    /// let ftdi = hal::Ft232hHal::new()?.init_default()?;
    /// let mut spi = ftdi.spi()?;
    /// # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
    /// ```
    pub fn spi(&self) -> Result<Spi, TimeoutError> {
        Spi::new(&self.mtx)
    }

    /// Aquire the I2C peripheral for the FT232H.
    ///
    /// Pin assignments:
    /// * AD0 => SCL
    /// * AD1 => SDA
    /// * AD2 => SDA
    ///
    /// Yes, AD1 and AD2 are both SDA.
    /// These pins must be shorted together for I2C operation.
    ///
    /// # Panics
    ///
    /// Panics if pin 0, 1, or 2 are already in use.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ftd2xx_embedded_hal as hal;
    ///
    /// let ftdi = hal::Ft232hHal::new()?.init_default()?;
    /// let mut i2c = ftdi.i2c()?;
    /// # Ok::<(), std::boxed::Box<dyn std::error::Error>>(())
    /// ```
    pub fn i2c(&self) -> Result<I2c, TimeoutError> {
        I2c::new(&self.mtx)
    }

    /// Aquire the digital output pin 0 for the FT232H.
    ///
    /// # Panics
    ///
    /// Panics if the pin is already in-use.
    pub fn ad0(&self) -> OutputPin {
        OutputPin::new(&self.mtx, 0)
    }

    /// Aquire the digital output pin 1 for the FT232H.
    ///
    /// # Panics
    ///
    /// Panics if the pin is already in-use.
    pub fn ad1(&self) -> OutputPin {
        OutputPin::new(&self.mtx, 1)
    }

    /// Aquire the digital output pin 2 for the FT232H.
    ///
    /// # Panics
    ///
    /// Panics if the pin is already in-use.
    pub fn ad2(&self) -> OutputPin {
        OutputPin::new(&self.mtx, 2)
    }

    /// Aquire the digital output pin 3 for the FT232H.
    ///
    /// # Panics
    ///
    /// Panics if the pin is already in-use.
    pub fn ad3(&self) -> OutputPin {
        OutputPin::new(&self.mtx, 3)
    }

    /// Aquire the digital output pin 4 for the FT232H.
    ///
    /// # Panics
    ///
    /// Panics if the pin is already in-use.
    pub fn ad4(&self) -> OutputPin {
        OutputPin::new(&self.mtx, 4)
    }

    /// Aquire the digital output pin 5 for the FT232H.
    ///
    /// # Panics
    ///
    /// Panics if the pin is already in-use.
    pub fn ad5(&self) -> OutputPin {
        OutputPin::new(&self.mtx, 5)
    }

    /// Aquire the digital output pin 6 for the FT232H.
    ///
    /// # Panics
    ///
    /// Panics if the pin is already in-use.
    pub fn ad6(&self) -> OutputPin {
        OutputPin::new(&self.mtx, 6)
    }

    /// Aquire the digital output pin 7 for the FT232H.
    ///
    /// # Panics
    ///
    /// Panics if the pin is already in-use.
    pub fn ad7(&self) -> OutputPin {
        OutputPin::new(&self.mtx, 7)
    }
}
