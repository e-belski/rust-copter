//! SPI interface for an MPU9250

use crate::{regs::*, Config, Error, Handle, Transport, MPU};
use embedded_hal::{blocking::delay::DelayMs, blocking::spi::Transfer};
use motion_sensor::{Accelerometer, Gyroscope, Magnetometer, Triplet};

use core::fmt::Debug;

const fn read(address: MPU9250) -> u16 {
    ((address as u16) | (1 << 7)) << 8
}

const fn write(address: MPU9250, value: u8) -> u16 {
    ((address as u16) << 8) | (value as u16)
}

impl<S> Transport for SPI<S>
where
    S: Transfer<u16>,
    S::Error: Debug,
{
    type Error = S::Error;
    fn mpu9250_read(&mut self, register: MPU9250) -> Result<u8, Error<Self::Error>> {
        let mut buffer = [read(register)];
        self.0
            .transfer(&mut buffer)
            .map(|buffer| {
                let value = (buffer[0] & 0xFF) as u8;
                log::trace!("READ {:?} => {:#04X}", register, value);
                value
            })
            .map_err(|err| {
                log::error!("READ {:?}: {:?}", register, err);
                err.into()
            })
    }
    fn mpu9250_write<B: Copy + Into<u8>>(
        &mut self,
        register: MPU9250,
        value: B,
    ) -> Result<(), Error<Self::Error>> {
        let value = value.into();
        let mut buffer = [write(register, value)];
        self.0
            .transfer(&mut buffer)
            .map(|_| {
                log::trace!("WRITE {:?} <= {:#04X}", register, value);
            })
            .map_err(|err| {
                log::error!("WRITE {:?}: {:?}", register, err);
                err.into()
            })
    }
    fn ak8963_read(&mut self, register: AK8963) -> Result<u8, Error<Self::Error>> {
        ak8963_read(&mut self.0, register)
            .map(|value| {
                log::trace!("READ {:?} => {:#04X}", register, value);
                value
            })
            .map_err(|err| {
                log::error!("READ {:?}: {:?}", register, err);
                err
            })
    }
    fn ak8963_write<B: Copy + Into<u8>>(
        &mut self,
        register: AK8963,
        value: B,
    ) -> Result<(), Error<Self::Error>> {
        ak8963_write(&mut self.0, register, value.into())
            .map(|_| {
                log::trace!("WRITE {:?} <= {:#04X}", register, value.into());
            })
            .map_err(|err| {
                log::error!("WRITE {:?}: {:?}", register, err);
                err
            })
    }
}

/// SPI communication transport for the MPU9250
pub struct SPI<S>(S);

/// Release a SPI-based MPU, returning the device handle
/// and the SPI peripheral
pub fn release<S>(mpu: MPU<SPI<S>>) -> (S, Handle) {
    (mpu.transport.0, mpu.handle)
}

/// Re-create the MPU from a SPI peripheral and an MPU `Handle`
///
/// Caller is reponsible for matching the peripheral to the handle.
/// Otherwise, we might be using the wrong handle for a different
/// physical MPU.
pub fn from_handle<S>(spi: S, handle: Handle) -> MPU<SPI<S>>
where
    S: Transfer<u16>,
{
    MPU {
        transport: SPI(spi),
        handle,
    }
}

/// Create a new SPI-based MPU
pub fn new<S>(
    spi: S,
    delay: &mut dyn DelayMs<u8>,
    config: &Config,
) -> Result<MPU<SPI<S>>, Error<S::Error>>
where
    S: Transfer<u16>,
    S::Error: Debug,
{
    let mut spi = SPI(spi);

    // Enable the I2C interface, just so we can power-down the AK8963...
    spi.mpu9250_write(MPU9250::USER_CTRL, USER_CTRL::I2C_MST_EN)?;
    spi.mpu9250_write(
        MPU9250::I2C_MST_CTRL,
        I2C_MST_CTRL::clock(I2C_MST_CLK::KHz400),
    )?;

    // Bring down both the AK8963 and the MPU9250
    spi.ak8963_write(AK8963::CNTL1, CNTL1::power_down())?;
    spi.mpu9250_write(MPU9250::PWR_MGMT_1, PWR_MGMT_1::reset())?;
    delay.delay_ms(10);

    // Re-enable the I2C interface.
    // Disable the I2C slave interface here, so that it doesn't think
    // we're talking to it as an I2C device.
    spi.mpu9250_write(
        MPU9250::USER_CTRL,
        (USER_CTRL::I2C_MST_EN | USER_CTRL::I2C_IF_DIS).bits(),
    )?;
    spi.mpu9250_write(
        MPU9250::I2C_MST_CTRL,
        I2C_MST_CTRL::clock(I2C_MST_CLK::KHz400),
    )?;

    // Soft-reset the AK8963
    spi.ak8963_write(AK8963::CNTL2, CNTL2::SRST.bits())?;

    // Set the gyro clock source
    spi.mpu9250_write(
        MPU9250::PWR_MGMT_1,
        PWR_MGMT_1::clock_select(PWR_MGMT_1_CLKSEL::AutoSelect),
    )?;

    // Sanity-check the WHO_AM_I values for both devices. By this point, we should be able
    // to address them.
    let who_am_i = spi.mpu9250_read(MPU9250::WHO_AM_I)?;
    if !mpu9250_regs::mpu9250::VALID_WHO_AM_I.contains(&who_am_i) {
        return Err(Error::WhoAmI {
            expected: mpu9250_regs::mpu9250::VALID_WHO_AM_I,
            actual: who_am_i,
        });
    }

    let who_am_i = spi.ak8963_read(AK8963::WIA)?;
    if !mpu9250_regs::ak8963::VALID_WHO_AM_I.contains(&who_am_i) {
        return Err(Error::WhoAmI {
            expected: mpu9250_regs::ak8963::VALID_WHO_AM_I,
            actual: who_am_i,
        });
    }

    let sensitivity = crate::mag_sensitivity(&mut spi, delay)?;

    // Apply user configuration
    config.apply(&mut spi)?;

    // Sample the AK8963 from the I2C_SLV0 controller
    //
    // After this runs, we'll need to disable the I2C_SLV0 polling to achieve
    // any configuration of the magnetometer.
    spi.mpu9250_write(MPU9250::I2C_SLV0_ADDR, AK8963_I2C_ADDRESS | (1 << 7))?;
    spi.mpu9250_write(MPU9250::I2C_SLV0_REG, AK8963::HXL as u8)?;
    spi.mpu9250_write(
        MPU9250::I2C_SLV0_CTRL,
        I2C_SLVX_CTRL {
            flags: I2C_SLVX_FLAGS::EN,
            length: 7,
        },
    )?;

    Ok(MPU::new(spi, &config, &sensitivity))
}

impl<S> Accelerometer<f64> for MPU<SPI<S>>
where
    S: Transfer<u16>,
{
    type Error = Error<S::Error>;
    fn accelerometer(&mut self) -> Result<Triplet<f64>, Self::Error> {
        const COMMANDS: [u16; 6] = [
            read(MPU9250::ACCEL_XOUT_H),
            read(MPU9250::ACCEL_XOUT_L),
            read(MPU9250::ACCEL_YOUT_H),
            read(MPU9250::ACCEL_YOUT_L),
            read(MPU9250::ACCEL_ZOUT_H),
            read(MPU9250::ACCEL_ZOUT_L),
        ];
        let mut buffer = COMMANDS;
        self.transport.0.transfer(&mut buffer)?;
        Ok(self.scale_acc(Triplet {
            x: ((buffer[0] << 8) | (buffer[1] & 0xFF)) as i16,
            y: ((buffer[2] << 8) | (buffer[3] & 0xFF)) as i16,
            z: ((buffer[4] << 8) | (buffer[5] & 0xFF)) as i16,
        }))
    }
}

impl<S> Gyroscope<f64> for MPU<SPI<S>>
where
    S: Transfer<u16>,
{
    type Error = Error<S::Error>;
    fn gyroscope(&mut self) -> Result<Triplet<f64>, Self::Error> {
        const COMMANDS: [u16; 6] = [
            read(MPU9250::GYRO_XOUT_H),
            read(MPU9250::GYRO_XOUT_L),
            read(MPU9250::GYRO_YOUT_H),
            read(MPU9250::GYRO_YOUT_L),
            read(MPU9250::GYRO_ZOUT_H),
            read(MPU9250::GYRO_ZOUT_L),
        ];
        let mut buffer = COMMANDS;
        self.transport.0.transfer(&mut buffer)?;
        Ok(self.scale_gyro(Triplet {
            x: ((buffer[0] << 8) | (buffer[1] & 0xFF)) as i16,
            y: ((buffer[2] << 8) | (buffer[3] & 0xFF)) as i16,
            z: ((buffer[4] << 8) | (buffer[5] & 0xFF)) as i16,
        }))
    }
}

impl<S> Magnetometer<f64> for MPU<SPI<S>>
where
    S: Transfer<u16>,
{
    type Error = Error<S::Error>;
    fn magnetometer(&mut self) -> Result<Triplet<f64>, Self::Error> {
        const COMMANDS: [u16; 6] = [
            read(MPU9250::EXT_SENS_DATA_00),
            read(MPU9250::EXT_SENS_DATA_01),
            read(MPU9250::EXT_SENS_DATA_02),
            read(MPU9250::EXT_SENS_DATA_03),
            read(MPU9250::EXT_SENS_DATA_04),
            read(MPU9250::EXT_SENS_DATA_05),
        ];
        let mut buffer = COMMANDS;
        self.transport.0.transfer(&mut buffer)?;
        Ok(self.scale_mag(Triplet {
            x: ((buffer[1] << 8) | (buffer[0] & 0xFF)) as i16,
            y: ((buffer[3] << 8) | (buffer[2] & 0xFF)) as i16,
            z: ((buffer[5] << 8) | (buffer[4] & 0xFF)) as i16,
        }))
    }
}

/// Read from the AK8963's register identified by `register`
fn ak8963_read<SPI: Transfer<u16>>(
    spi: &mut SPI,
    register: AK8963,
) -> Result<u8, Error<SPI::Error>> {
    spi.transfer(&mut [write(MPU9250::I2C_SLV4_ADDR, AK8963_I2C_ADDRESS | (1 << 7))])?;
    spi.transfer(&mut [write(MPU9250::I2C_SLV4_REG, register as u8)])?;
    spi.transfer(&mut [write(
        MPU9250::I2C_SLV4_CTRL,
        I2C_SLV4_CTRL::I2C_SLV4_EN.bits(),
    )])?;
    ak8963_wait_done(spi, 10_000, register, None)?;

    let mut buffer = [read(MPU9250::I2C_SLV4_DI)];
    spi.transfer(&mut buffer)?;
    Ok((buffer[0] & 0xFF) as u8)
}

/// Write's `value` to the AK8963's `register`
fn ak8963_write<SPI: Transfer<u16>>(
    spi: &mut SPI,
    register: AK8963,
    value: u8,
) -> Result<(), Error<SPI::Error>> {
    spi.transfer(&mut [write(MPU9250::I2C_SLV4_ADDR, AK8963_I2C_ADDRESS)])?;
    spi.transfer(&mut [write(MPU9250::I2C_SLV4_REG, register as u8)])?;
    spi.transfer(&mut [write(MPU9250::I2C_SLV4_DO, value)])?;
    spi.transfer(&mut [write(
        MPU9250::I2C_SLV4_CTRL,
        I2C_SLV4_CTRL::I2C_SLV4_EN.bits(),
    )])?;
    ak8963_wait_done(spi, 10_000, register, Some(value))?;
    Ok(())
}

/// Wait `max_attempts` for the indication that the I2C transation is complete
fn ak8963_wait_done<SPI: Transfer<u16>>(
    spi: &mut SPI,
    max_attempts: u16,
    register: AK8963,
    value: Option<u8>,
) -> Result<(), Error<SPI::Error>> {
    for _ in 0..max_attempts {
        let mut buffer = [read(MPU9250::I2C_MST_STATUS)];
        spi.transfer(&mut buffer)?;
        let status = I2C_MST_STATUS::from_bits_truncate((buffer[0] & 0xFF) as u8);
        if status.contains(I2C_MST_STATUS::I2C_SLV4_DONE) {
            return Ok(());
        } else if status.contains(I2C_MST_STATUS::I2C_SLV4_NACK) {
            return Err(Error::Nack);
        } else if status.contains(I2C_MST_STATUS::I2C_LOST_ARB) {
            return Err(Error::LostArbitration);
        }
    }
    Err(Error::Timeout {
        attempts: max_attempts,
        register,
        value,
    })
}
