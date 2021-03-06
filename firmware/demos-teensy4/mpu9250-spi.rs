//! Pinout:
//!
//! - Teensy 4 Pin 13 (SCK) to MPU's SCL (Note that we lose the LED here)
//! - Teensy 4 Pin 11 (MOSI) to MPU's SDA/SDI
//! - Teensy 4 Pin 12 (MISO) to MPU's AD0/SDO
//! - Teensy 4 Pin 10 (PSC0) to MPU's NCS

#![no_std]
#![no_main]
// This seems to be a bad lint. The patter is valid
// for a non_exhaustive struct.
#![allow(clippy::field_reassign_with_default)]

use shared as _;

use cortex_m_rt::entry;
use motion_sensor::*;
use teensy4_bsp as bsp;

const SPI_BAUD_RATE_HZ: u32 = 1_000_000;

#[entry]
fn main() -> ! {
    let mut peripherals = bsp::Peripherals::take().unwrap();
    let core_peripherals = cortex_m::Peripherals::take().unwrap();
    let mut systick = bsp::SysTick::new(core_peripherals.SYST);
    let pins = bsp::t40::into_pins(peripherals.iomuxc);

    bsp::usb::init(
        &systick,
        bsp::usb::LoggingConfig {
            filters: &[],
            ..Default::default()
        },
    )
    .unwrap();

    peripherals.ccm.pll1.set_arm_clock(
        bsp::hal::ccm::PLL1::ARM_HZ,
        &mut peripherals.ccm.handle,
        &mut peripherals.dcdc,
    );

    systick.delay(5000);
    log::info!("Initializing SPI4 clocks...");

    let (_, _, _, spi4_builder) = peripherals.spi.clock(
        &mut peripherals.ccm.handle,
        bsp::hal::ccm::spi::ClockSelect::Pll2,
        bsp::hal::ccm::spi::PrescalarSelect::LPSPI_PODF_5,
    );

    log::info!("Constructing SPI4 peripheral...");
    let mut spi4 = spi4_builder.build(pins.p11, pins.p12, pins.p13);

    match spi4.set_clock_speed(bsp::hal::spi::ClockSpeed(SPI_BAUD_RATE_HZ)) {
        Ok(()) => {
            log::info!("Set clock speed to {}Hz", SPI_BAUD_RATE_HZ);
        }
        Err(err) => {
            panic!(
                "Unable to set clock speed to {}Hz: {:?}",
                SPI_BAUD_RATE_HZ, err
            );
        }
    };

    spi4.enable_chip_select_0(pins.p10);
    log::info!("Waiting a few seconds before querying MPU9250...");
    systick.delay(4000);

    let mut config = invensense_mpu::Config::default();
    config.accel_scale = invensense_mpu::regs::ACCEL_FS_SEL::G8;
    config.mag_control = invensense_mpu::regs::CNTL1 {
        mode: invensense_mpu::regs::CNTL1_MODE::CONTINUOUS_2,
        ..Default::default()
    };
    let mut sensor = match invensense_mpu::spi::new(spi4, &mut systick, &config) {
        Ok(sensor) => sensor,
        Err(err) => {
            panic!("Error when constructing MP9250: {:?}", err);
        }
    };

    log::info!(
        "MPU9250 WHO_AM_I = {:#X}",
        sensor.mpu9250_who_am_i().unwrap()
    );
    log::info!("AK8963 WHO_AM_I = {:#X}", sensor.ak8963_who_am_i().unwrap());
    systick.delay(5000);
    loop {
        log::info!("ACC {:?}", sensor.accelerometer().unwrap());
        log::info!("GYRO {:?}", sensor.gyroscope().unwrap());
        log::info!("MAG {:?}", sensor.magnetometer().unwrap());

        systick.delay(250);
    }
}
