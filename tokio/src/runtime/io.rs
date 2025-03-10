pub(crate) use self::variant::*;

/// Re-exported for convenience.
pub(crate) use std::io::Result;

#[cfg(feature = "net-driver")]
mod variant {
    use crate::net::driver;

    use std::io;

    /// The driver value the runtime passes to the `timer` layer.
    ///
    /// When the `io-driver` feature is enabled, this is the "real" I/O driver
    /// backed by Mio. Without the `io-driver` feature, this is a thread parker
    /// backed by a condition variable.
    pub(crate) type Driver = driver::Reactor;

    /// The handle the runtime stores for future use.
    ///
    /// When the `io-driver` feature is **not** enabled, this is `()`.
    pub(crate) type Handle = driver::Handle;

    pub(crate) fn create() -> io::Result<(Driver, Handle)> {
        let driver = driver::Reactor::new()?;
        let handle = driver.handle();

        Ok((driver, handle))
    }

    pub(crate) fn set_default(handle: &Handle) -> driver::DefaultGuard<'_> {
        driver::set_default(handle)
    }
}

#[cfg(not(feature = "net-driver"))]
mod variant {
    use crate::runtime::park::ParkThread;

    use std::io;

    /// I/O is not enabled, use a condition variable based parker
    pub(crate) type Driver = ParkThread;

    /// There is no handle
    pub(crate) type Handle = ();

    pub(crate) fn create() -> io::Result<(Driver, Handle)> {
        let driver = ParkThread::new();

        Ok((driver, ()))
    }

    #[cfg(feature = "blocking")]
    pub(crate) fn set_default(_handle: &Handle) {}
}
