use std::{
	sync::{
		atomic::{AtomicBool, Ordering},
		Mutex,
	},
	thread,
	time::Duration,
};

use crate::{ActorUnit, Unit};

// =========================================================================

// Because of statics we must run tests sequentially.
static SEMAPHORE: Mutex<()> = Mutex::new(());

static SHARED_DATA: Mutex<Option<()>> = Mutex::new(None);
static SHOULD_STOP: AtomicBool = AtomicBool::new(false);
static CANNOT_STOP: AtomicBool = AtomicBool::new(false);

fn cleanup() {
	*SHARED_DATA.lock().unwrap() = None;
	SHOULD_STOP.store(false, Ordering::Relaxed);
	CANNOT_STOP.store(false, Ordering::Relaxed);
}

fn example_state_setup() {
	// Sleep to reproduce potential race easily.
	thread::sleep(Duration::from_millis(100));
	*SHARED_DATA.lock().unwrap() = Some(());
}

fn example_start() {
	loop {
		let lock = SHARED_DATA.lock().unwrap();
		let Some(_) = lock.as_ref() else {
			break;
		};
		// Here we are still holding lock on the state,
		// which does not tell us to stop. However, flag
		// says that we must, so mark error and break.
		if SHOULD_STOP.load(Ordering::Relaxed) {
			CANNOT_STOP.store(true, Ordering::Relaxed);
			break;
		}
		drop(lock);
		let dur = Duration::from_millis(100);
		thread::sleep(dur);
	}
}

// =========================================================================

struct RaceWithoutSetupActor;

impl super::Actor for RaceWithoutSetupActor {
	unsafe fn spawn(
		f: extern "C" fn(*const crate::ActorUnit<Self>),
		s: &'static crate::ActorUnit<Self>,
	) {
		thread::spawn(move || f(s));
	}

	fn start(&self) {
		example_state_setup();
		example_start();
	}

	fn abort(&self) {
		*SHARED_DATA.lock().unwrap() = None;
		SHOULD_STOP.store(true, Ordering::Relaxed);
	}
}

static RACE_WITHOUT_SETUP_ACTOR_UNIT: ActorUnit<RaceWithoutSetupActor> =
	ActorUnit::new(RaceWithoutSetupActor);

#[test]
fn check_race_without_setup() {
	let lock = SEMAPHORE.lock().unwrap();
	cleanup();
	unsafe { RACE_WITHOUT_SETUP_ACTOR_UNIT.acquire() };
	unsafe { RACE_WITHOUT_SETUP_ACTOR_UNIT.release() };
	assert!(CANNOT_STOP.load(Ordering::Relaxed));
	drop(lock);
}

// =========================================================================

struct NoRaceWithSetupActor;

impl super::Actor for NoRaceWithSetupActor {
	unsafe fn spawn(
		f: extern "C" fn(*const crate::ActorUnit<Self>),
		s: &'static crate::ActorUnit<Self>,
	) {
		thread::spawn(move || f(s));
	}

	fn setup(&self) {
		example_state_setup();
	}

	fn start(&self) {
		example_start();
	}

	fn abort(&self) {
		*SHARED_DATA.lock().unwrap() = None;
		SHOULD_STOP.store(true, Ordering::Relaxed);
	}
}

static NO_RACE_WITH_SETUP_ACTOR_UNIT: ActorUnit<NoRaceWithSetupActor> =
	ActorUnit::new(NoRaceWithSetupActor);

#[test]
fn check_no_race_with_setup() {
	let lock = SEMAPHORE.lock().unwrap();
	cleanup();
	unsafe { NO_RACE_WITH_SETUP_ACTOR_UNIT.acquire() };
	unsafe { NO_RACE_WITH_SETUP_ACTOR_UNIT.release() };
	assert!(!CANNOT_STOP.load(Ordering::Relaxed));
	drop(lock);
}
