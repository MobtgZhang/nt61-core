//! Owned routing prerequisite for private file I/O. This pins the registered
//! driver and its device allocation; it does not resolve a volume/PDO, retain a
//! VPB, freeze attachment topology, or implement a native power syscall.
use super::{device_reference::DeviceReference, driver_loader::DriverReference, DeviceObject};

#[must_use]
pub struct FileDeviceBinding {
    // Rust drops fields in declaration order: release the device first, keeping
    // its owner/image retained throughout that release.
    device: DeviceReference,
    driver: DriverReference,
}

impl FileDeviceBinding {
    /// Find and retain the loaded driver atomically, then retain its device
    /// under the graph lock. Failure drops the driver guard automatically.
    pub fn find(driver_name: &[u8]) -> Result<Self, i32> {
        let driver = DriverReference::find(driver_name)?;
        let device = DeviceReference::for_driver(&driver)?;
        Ok(Self { device, driver })
    }

    pub fn device(&self) -> &DeviceReference {
        &self.device
    }

    pub fn device_ptr(&self) -> *mut DeviceObject {
        self.device.as_ptr()
    }

    pub fn driver(&self) -> &DriverReference {
        &self.driver
    }

    pub fn try_clone(&self) -> Result<Self, i32> {
        let driver = self.driver.try_clone()?;
        // Clone the original device, rather than resolving a possibly new head.
        let device = self.device.try_clone()?;
        Ok(Self { device, driver })
    }
}
