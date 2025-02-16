use core::{
	mem::ManuallyDrop,
	sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::{sync::Mutex, thread};

// =========================================================================

pub trait Actor
where
	Self: Sized,
{
	unsafe fn spawn(
		f: extern "C" fn(*const ActorUnit<Self>),
		s: &'static ActorUnit<Self>,
	);
	fn start(&self);
	fn abort(&self);
}

pub trait Unit {
	/// SAFETY: Acquire must be called as many times as release.
	unsafe fn acquire(&'static self);
	/// SAFETY: Release must be called as many times as acquire.
	unsafe fn release(&'static self);
}

impl<T1: Unit> Unit for &T1 {
	unsafe fn acquire(&'static self) {
		T1::acquire(self);
	}

	unsafe fn release(&'static self) {
		T1::release(self);
	}
}

// =========================================================================

pub struct ActorUnit<A1>
where
	A1: Actor,
{
	// Flag to check state of actor.
	running: AtomicBool,

	// Semaphore to avoid races.
	semaphore: Mutex<()>,

	// Count for dependent actors.
	count: AtomicUsize,

	// Encapsulated actor.
	actor: A1,
}

impl<A1> ActorUnit<A1>
where
	A1: Actor,
{
	pub const fn new(inner: A1) -> Self {
		Self {
			running: AtomicBool::new(false),
			semaphore: Mutex::new(()),
			count: AtomicUsize::new(0usize),
			actor: inner,
		}
	}

	fn spawn(&'static self) {
		// SAFETY: Self is borrowed for 'static so pointer will be valid.
		unsafe { A1::spawn(Self::start, self) };
		// Yield to scheduler till spawned actor started.
		while !self.running.load(Ordering::Relaxed) {
			thread::yield_now();
		}
	}

	fn running(&self) {
		self.running.store(true, Ordering::Relaxed);
	}

	extern "C" fn start(s: *const Self) {
		// SAFETY: Assuming Self::spawn called us with right pointer.
		let this = unsafe { s.as_ref() }.expect("is not null");
		// Set running and call start that user provided.
		(this.running(), this.actor.start());
		// Check counter to know if return is intentional or not.
		// We do not support non intentional exit yet so panic.
		if this.count.load(Ordering::Relaxed) != 0 {
			panic!("actor exited early");
		}
		// We set it after check so that guard is not released.
		this.running.store(false, Ordering::Relaxed);
	}

	fn abort(&self) {
		// Just forward to impl provided by user and then wait.
		self.actor.abort();
		// Yield to scheduler till spawned actor stopped.
		while self.running.load(Ordering::Relaxed) {
			thread::yield_now();
		}
	}
}

impl<A1> Unit for ActorUnit<A1>
where
	A1: Actor,
{
	unsafe fn acquire(&'static self) {
		let guard = ManuallyDrop::new(self.semaphore.lock().unwrap());
		let spawn = || self.spawn();
		(self.count.fetch_add(1, Ordering::Relaxed) == 0).then(spawn);
		drop(ManuallyDrop::into_inner(guard));
	}

	unsafe fn release(&'static self) {
		let guard = ManuallyDrop::new(self.semaphore.lock().unwrap());
		let abort = || self.abort();
		(self.count.fetch_sub(1, Ordering::Relaxed) == 1).then(abort);
		drop(ManuallyDrop::into_inner(guard));
	}
}

// =========================================================================

impl<U1, U2> Unit for (U1, U2)
where
	U1: Unit,
	U2: Unit,
{
	unsafe fn acquire(&'static self) {
		unsafe { self.0.acquire() };
		unsafe { self.1.acquire() };
	}

	unsafe fn release(&'static self) {
		unsafe { self.0.release() };
		unsafe { self.1.release() };
	}
}

// =========================================================================

pub struct Blueprint<U1: Unit>(U1);

impl<U1> Blueprint<U1>
where
	U1: Unit,
{
	pub const fn new(unit: U1) -> Blueprint<U1> {
		Blueprint(unit)
	}

	pub const fn register<U2>(self, unit: U2) -> Blueprint<(U2, Self)>
	where
		U2: Unit,
	{
		Blueprint((unit, self))
	}
}

impl<U1> Unit for Blueprint<U1>
where
	U1: Unit,
{
	unsafe fn acquire(&'static self) {
		unsafe { self.0.acquire() };
	}

	unsafe fn release(&'static self) {
		unsafe { self.0.release() };
	}
}
