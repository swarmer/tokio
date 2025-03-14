//! A batteries included runtime for applications using Tokio.
//!
//! Applications using Tokio require some runtime support in order to work:
//!
//! * A [driver] to drive I/O resources.
//! * An [executor] to execute tasks that use these I/O resources.
//! * A [timer] for scheduling work to run after a set period of time.
//!
//! While it is possible to setup each component manually, this involves a bunch
//! of boilerplate.
//!
//! [`Runtime`] bundles all of these various runtime components into a single
//! handle that can be started and shutdown together, eliminating the necessary
//! boilerplate to run a Tokio application.
//!
//! Most applications wont need to use [`Runtime`] directly. Instead, they will
//! use the [`tokio::main`] attribute macro, which uses [`Runtime`] under the hood.
//!
//! Creating a [`Runtime`] does the following:
//!
//! * Spawn a background thread running a [`Reactor`] instance.
//! * Start a thread pool for executing futures.
//! * Run an instance of `Timer` **per** thread pool worker thread.
//!
//! The thread pool uses a work-stealing strategy and is configured to start a
//! worker thread for each CPU core available on the system. This tends to be
//! the ideal setup for Tokio applications.
//!
//! A timer per thread pool worker thread is used to minimize the amount of
//! synchronization that is required for working with the timer.
//!
//! # Usage
//!
//! Most applications will use the [`tokio::main`] attribute macro.
//!
//! ```no_run
//! use tokio::net::TcpListener;
//! use tokio::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut listener = TcpListener::bind("127.0.0.1:8080").await?;
//!
//!     loop {
//!         let (mut socket, _) = listener.accept().await?;
//!
//!         tokio::spawn(async move {
//!             let mut buf = [0; 1024];
//!
//!             // In a loop, read data from the socket and write the data back.
//!             loop {
//!                 let n = match socket.read(&mut buf).await {
//!                     // socket closed
//!                     Ok(n) if n == 0 => return,
//!                     Ok(n) => n,
//!                     Err(e) => {
//!                         println!("failed to read from socket; err = {:?}", e);
//!                         return;
//!                     }
//!                 };
//!
//!                 // Write the data back
//!                 if let Err(e) = socket.write_all(&buf[0..n]).await {
//!                     println!("failed to write to socket; err = {:?}", e);
//!                     return;
//!                 }
//!             }
//!         });
//!     }
//! }
//! ```
//!
//! From within the context of the runtime, additional tasks are spawned using
//! the [`tokio::spawn`] function. Futures spawned using this function will be
//! executed on the same thread pool used by the [`Runtime`].
//!
//! A [`Runtime`] instance can also be used directly.
//!
//! ```no_run
//! use tokio::net::TcpListener;
//! use tokio::prelude::*;
//! use tokio::runtime::Runtime;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create the runtime
//!     let mut rt = Runtime::new()?;
//!
//!     // Spawn the root task
//!     rt.block_on(async {
//!         let mut listener = TcpListener::bind("127.0.0.1:8080").await?;
//!
//!         loop {
//!             let (mut socket, _) = listener.accept().await?;
//!
//!             tokio::spawn(async move {
//!                 let mut buf = [0; 1024];
//!
//!                 // In a loop, read data from the socket and write the data back.
//!                 loop {
//!                     let n = match socket.read(&mut buf).await {
//!                         // socket closed
//!                         Ok(n) if n == 0 => return,
//!                         Ok(n) => n,
//!                         Err(e) => {
//!                             println!("failed to read from socket; err = {:?}", e);
//!                             return;
//!                         }
//!                     };
//!
//!                     // Write the data back
//!                     if let Err(e) = socket.write_all(&buf[0..n]).await {
//!                         println!("failed to write to socket; err = {:?}", e);
//!                         return;
//!                     }
//!                 }
//!             });
//!         }
//!     })
//! }
//! ```
//!
//! [driver]: tokio::net::driver
//! [executor]: https://tokio.rs/docs/internals/runtime-model/#executors
//! [timer]: ../timer/index.html
//! [`Runtime`]: struct.Runtime.html
//! [`Reactor`]: ../reactor/struct.Reactor.html
//! [`run`]: fn.run.html
//! [`tokio::spawn`]: ../executor/fn.spawn.html
//! [`tokio::main`]: ../../tokio_macros/attr.main.html

// At the top due to macros
#[cfg(test)]
#[macro_use]
mod tests;

#[cfg(all(not(feature = "blocking"), feature = "rt-full"))]
mod blocking;
#[cfg(feature = "blocking")]
pub mod blocking;
#[cfg(feature = "blocking")]
use crate::runtime::blocking::PoolWaiter;

mod builder;
pub use self::builder::Builder;

#[cfg(feature = "rt-current-thread")]
mod current_thread;
#[cfg(feature = "rt-current-thread")]
use self::current_thread::CurrentThread;

#[cfg(feature = "blocking")]
mod enter;
#[cfg(feature = "blocking")]
use self::enter::enter;

mod global;
pub use self::global::spawn;

mod io;

mod park;
pub use self::park::{Park, Unpark};

#[cfg(feature = "rt-current-thread")]
mod spawner;
#[cfg(feature = "rt-current-thread")]
pub use self::spawner::Spawner;

#[cfg(feature = "rt-current-thread")]
mod task;
#[cfg(feature = "rt-current-thread")]
pub use self::task::{JoinError, JoinHandle};

mod timer;

#[cfg(feature = "rt-full")]
pub(crate) mod thread_pool;
#[cfg(feature = "rt-full")]
use self::thread_pool::ThreadPool;

#[cfg(feature = "blocking")]
use std::future::Future;

/// The Tokio runtime, includes a reactor as well as an executor for running
/// tasks.
///
/// Instances of `Runtime` can be created using [`new`] or [`Builder`]. However,
/// most users will use the `#[tokio::main]` annotation on their entry point.
///
/// See [module level][mod] documentation for more details.
///
/// # Shutdown
///
/// Shutting down the runtime is done by dropping the value. The current thread
/// will block until the shut down operation has completed.
///
/// * Drain any scheduled work queues.
/// * Drop any futures that have not yet completed.
/// * Drop the reactor.
///
/// Once the reactor has dropped, any outstanding I/O resources bound to
/// that reactor will no longer function. Calling any method on them will
/// result in an error.
///
/// [mod]: index.html
/// [`new`]: #method.new
/// [`Builder`]: struct.Builder.html
/// [`tokio::run`]: fn.run.html
#[derive(Debug)]
pub struct Runtime {
    /// Task executor
    kind: Kind,

    /// Handles to the network drivers
    net_handles: Vec<io::Handle>,

    /// Timer handles
    timer_handles: Vec<timer::Handle>,

    /// Blocking pool handle
    #[cfg(feature = "blocking")]
    blocking_pool: PoolWaiter,
}

/// The runtime executor is either a thread-pool or a current-thread executor.
#[derive(Debug)]
enum Kind {
    /// Not able to execute concurrent tasks. This variant is mostly used to get
    /// access to the driver handles.
    Shell,

    /// Execute all tasks on the current-thread.
    #[cfg(feature = "rt-current-thread")]
    CurrentThread(CurrentThread<timer::Driver>),

    /// Execute tasks across multiple threads.
    #[cfg(feature = "rt-full")]
    ThreadPool(ThreadPool),
}

impl Runtime {
    /// Create a new runtime instance with default configuration values.
    ///
    /// This results in a reactor, thread pool, and timer being initialized. The
    /// thread pool will not spawn any worker threads until it needs to, i.e.
    /// tasks are scheduled to run.
    ///
    /// Most users will not need to call this function directly, instead they
    /// will use [`tokio::run`](fn.run.html).
    ///
    /// See [module level][mod] documentation for more details.
    ///
    /// # Examples
    ///
    /// Creating a new `Runtime` with default configuration values.
    ///
    /// ```
    /// use tokio::runtime::Runtime;
    ///
    /// let rt = Runtime::new()
    ///     .unwrap();
    ///
    /// // Use the runtime...
    /// ```
    ///
    /// [mod]: index.html
    pub fn new() -> io::Result<Self> {
        #[cfg(feature = "rt-full")]
        let ret = Builder::new().thread_pool().build();

        #[cfg(all(not(feature = "rt-full"), feature = "rt-current-thread"))]
        let ret = Builder::new().current_thread().build();

        #[cfg(not(feature = "rt-current-thread"))]
        let ret = Builder::new().build();

        ret
    }

    /// Spawn a future onto the Tokio runtime.
    ///
    /// This spawns the given future onto the runtime's executor, usually a
    /// thread pool. The thread pool is then responsible for polling the future
    /// until it completes.
    ///
    /// See [module level][mod] documentation for more details.
    ///
    /// [mod]: index.html
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::runtime::Runtime;
    ///
    /// # fn dox() {
    /// // Create the runtime
    /// let rt = Runtime::new().unwrap();
    ///
    /// // Spawn a future onto the runtime
    /// rt.spawn(async {
    ///     println!("now running on a worker thread");
    /// });
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// This function panics if the spawn fails. Failure occurs if the executor
    /// is currently at capacity and is unable to spawn a new future.
    #[cfg(feature = "rt-current-thread")]
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        match &self.kind {
            Kind::Shell => panic!("task execution disabled"),
            #[cfg(feature = "rt-full")]
            Kind::ThreadPool(exec) => exec.spawn(future),
            Kind::CurrentThread(exec) => exec.spawn(future),
        }
    }

    /// Run a future to completion on the Tokio runtime. This is the runtime's
    /// entry point.
    ///
    /// This runs the given future on the runtime, blocking until it is
    /// complete, and yielding its resolved result. Any tasks or timers which
    /// the future spawns internally will be executed on the runtime.
    ///
    /// This method should not be called from an asynchronous context.
    ///
    /// # Panics
    ///
    /// This function panics if the executor is at capacity, if the provided
    /// future panics, or if called within an asynchronous execution context.
    #[cfg(feature = "blocking")] // TODO: remove this
    pub fn block_on<F: Future>(&mut self, future: F) -> F::Output {
        let _net = io::set_default(&self.net_handles[0]);
        let _timer = timer::set_default(&self.timer_handles[0]);

        let kind = &mut self.kind;

        blocking::with_pool(&self.blocking_pool, || match kind {
            Kind::Shell => enter().block_on(future),
            #[cfg(feature = "rt-current-thread")]
            Kind::CurrentThread(exec) => exec.block_on(future),
            #[cfg(feature = "rt-full")]
            Kind::ThreadPool(exec) => exec.block_on(future),
        })
    }

    /// Return a handle to the runtime's spawner.
    ///
    /// The returned handle can be used to spawn tasks that run on this runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::runtime::Runtime;
    ///
    /// let rt = Runtime::new()
    ///     .unwrap();
    ///
    /// let spawner = rt.spawner();
    ///
    /// spawner.spawn(async { println!("hello"); });
    /// ```
    #[cfg(feature = "rt-current-thread")]
    pub fn spawner(&self) -> Spawner {
        match &self.kind {
            Kind::Shell => Spawner::shell(),
            #[cfg(feature = "rt-current-thread")]
            Kind::CurrentThread(exec) => Spawner::current_thread(exec.spawner()),
            #[cfg(feature = "rt-full")]
            Kind::ThreadPool(exec) => Spawner::thread_pool(exec.spawner().clone()),
        }
    }
}
