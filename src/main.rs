use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering, AtomicBool};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::{Result, eyre::eyre};
use rppal::uart::*;
use serde::Serialize;
use serde_json::json;
use simple_prometheus_exporter::{Exporter, export};
use sps30::Sps30;
use structopt::StructOpt;
use tracing::{instrument, info, error, warn, debug};
use warp::Filter;

mod util;

const MASS_UNIT: &str = "μg/m3";
const NUMBER_UNIT: &str = "1/cm3";
const PARTICLE_SIZE_UNIT: &str = "μm";

/// A safe read interval. New values are ostensibly available every 1s. We
/// double this avoid repeatedly falling into some "data unavailable" loop since
/// even 2s is much faster than we'll be polling.
const READ_INTERVAL: Duration = Duration::from_millis(2000);

/// Device startup time, per the datasheet.
const STARTUP_DURATION: Duration = Duration::from_millis(8000);

#[derive(Debug, Clone, StructOpt)]
#[structopt(name = "sps30-exporter")]
struct Options {
  /// sensor serial device, e.g. /dev/ttyUSB0
  #[structopt(parse(from_os_str))]
  device: PathBuf,

  /// HTTP server port
  #[structopt(long, short, default_value = "8090", env = "SPS30_PORT")]
  port: u16,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct MassConcentration {
  /// PM1.0, in μg/m3
  pub pm1: f32,

  /// PM2.5, in μg/m3
  pub pm25: f32,

  /// PM4, in μg/m3
  pub pm4: f32,

  /// PM10, in μg/m3
  pub pm10: f32,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct NumberConcentration {
  /// PM0.5, in 1/cm3
  pub pm05: f32,

  /// PM1.0, in 1/cm3
  pub pm1: f32,

  /// PM2.5, in 1/cm3
  pub pm25: f32,

  /// PM4, in 1/cm3
  pub pm4: f32,

  /// PM10, in 1/cm3
  pub pm10: f32,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct Measurement {
  pub mass: MassConcentration,
  pub number: NumberConcentration,

  /// The typical particle size, in μm
  pub typical_particle_size: f32,
}

impl Measurement {
  fn from_array(arr: [f32; 10]) -> Measurement {
    Measurement {
      mass: MassConcentration {
        pm1: arr[0],
        pm25: arr[1],
        pm4: arr[2],
        pm10: arr[3],
      },
      number: NumberConcentration {
        pm05: arr[4],
        pm1: arr[5],
        pm25: arr[6],
        pm4: arr[7],
        pm10: arr[8],
      },
      typical_particle_size: arr[9],
    }
  }
}

fn map_sps30_error<E, F>(e: sps30::Error<E, F>) -> color_eyre::eyre::Error
where
  E: std::fmt::Debug,
  F: std::fmt::Debug
{
  // bleh.
  eyre!("{:?}", e)
}

#[instrument(skip_all)]
fn read_thread(
  reading_lock: Arc<RwLock<Option<Measurement>>>,
  error_count: Arc<AtomicUsize>,
  term: Arc<AtomicBool>,
  opts: &Options
) -> Result<()> {
  let mut serial = Uart::with_path(&opts.device, 115_200, Parity::None, 8, 1)?;
  serial.set_hardware_flow_control(false)?;
  serial.set_software_flow_control(false)?;
  serial.set_rts(false)?;
  serial.set_write_mode(true)?;
  serial.set_read_mode(1, Duration::new(0, 0))?;

  let mut sps30 = Sps30::new(serial);
  sps30.reset().map_err(map_sps30_error)?;

  // Per the datasheet, the sensor takes 8 seconds to initialize.
  thread::sleep(STARTUP_DURATION);

  sps30.start_measurement().map_err(map_sps30_error)?;

  let mut last_read = Instant::now();
  loop {
    if term.load(Ordering::Relaxed) {
      break;
    }

    if let Some(sleep_duration) = READ_INTERVAL.checked_sub(last_read.elapsed()) {
      thread::sleep(sleep_duration);
    }

    last_read = Instant::now();

    match sps30.read_measurement() {
      Ok(data) => {
        let m = Measurement::from_array(data);

        let mut lock = match reading_lock.write() {
          Ok(lock) => lock,
          Err(e) => {
            warn!("could not acquire lock: {:?}", e);
            error_count.fetch_add(1, Ordering::Relaxed);
            continue;
          }
        };

        *lock = Some(m);
      },

      // Do nothing on an empty result.
      Err(sps30::Error::EmptyResult) => {
        debug!("Received empty result.")
      },

      Err(e) => {
        warn!("Read error: {:?}", e);
        error_count.fetch_add(1, Ordering::Relaxed);
      }
    }
  }

  if let Err(e) = sps30.stop_measurement() {
    error!("could not stop measurements: {:?}", e);
  }

  std::process::exit(0);
}

fn export_measurement(
  exporter: &Exporter,
  measurement: Option<Measurement>,
  error_count: &Arc<AtomicUsize>,
  fatal_error_count: &Arc<AtomicUsize>
) -> String {
  let mut s = exporter.session();

  match measurement {
    Some(r) => {
      export!(s, "sps30_mass_concentration", r.mass.pm1, variant = "PM1.0", unit = MASS_UNIT);
      export!(s, "sps30_mass_concentration", r.mass.pm25, variant = "PM2.5", unit = MASS_UNIT);
      export!(s, "sps30_mass_concentration", r.mass.pm4, variant = "PM4", unit = MASS_UNIT);
      export!(s, "sps30_mass_concentration", r.mass.pm10, variant = "PM10", unit = MASS_UNIT);

      export!(s, "sps30_number_concentration", r.number.pm05, variant = "PM0.5", unit = NUMBER_UNIT);
      export!(s, "sps30_number_concentration", r.number.pm1, variant = "PM1.0", unit = NUMBER_UNIT);
      export!(s, "sps30_number_concentration", r.number.pm25, variant = "PM2.5", unit = NUMBER_UNIT);
      export!(s, "sps30_number_concentration", r.number.pm4, variant = "PM4", unit = NUMBER_UNIT);
      export!(s, "sps30_number_concentration", r.number.pm10, variant = "PM10", unit = NUMBER_UNIT);

      export!(s, "sps30_typical_particle_size", r.typical_particle_size, unit = PARTICLE_SIZE_UNIT);
    },
    None => ()
  };

  export!(s, "sps30_error_count", error_count.load(Ordering::Relaxed) as f64);
  export!(s, "sps30_fatal_error_count", fatal_error_count.load(Ordering::Relaxed) as f64);

  s.to_string()
}

#[instrument]
#[tokio::main]
async fn main() -> Result<()> {
  util::install_tracing();
  color_eyre::install()?;

  let opts = Options::from_args();
  let port = opts.port;

  let latest_reading_lock = Arc::new(RwLock::new(None));
  let error_count = Arc::new(AtomicUsize::new(0));
  let fatal_error_count = Arc::new(AtomicUsize::new(0));

  let term = Arc::new(AtomicBool::new(false));
  signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;

  let thread_reading = Arc::clone(&latest_reading_lock);
  let thread_error_count = Arc::clone(&error_count);
  let thread_fatal_error_count = Arc::clone(&fatal_error_count);
  let thread_handle = thread::spawn(move || {
    if let Err(e) = read_thread(thread_reading, thread_error_count, term, &opts) {
      error!("read thread failed: {}", e);
      thread_fatal_error_count.fetch_add(1, Ordering::Relaxed);
    }
  });

  let thread_handle_task = tokio::task::spawn_blocking(move || {
    thread_handle.join()
  });

  let json_lock = Arc::clone(&latest_reading_lock);
  let r_json = warp::path("json").map(move || {
    match *json_lock.read().unwrap() {
      Some(ref r) => warp::reply::json(r),
      None => warp::reply::json(&json!(null))
    }
  });

  let exporter = Arc::new(Exporter::new());
  let metrics_lock = Arc::clone(&latest_reading_lock);
  let metrics_error_count = Arc::clone(&error_count);
  let metrics_fatal_error_count = Arc::clone(&fatal_error_count);
  let r_metrics = warp::path("metrics").map(move || {
    export_measurement(
      &exporter,
      *metrics_lock.read().unwrap(),
      &metrics_error_count,
      &metrics_fatal_error_count
    )
  });

  info!("starting exporter on port {}", port);

  let routes = warp::get().and(r_json).or(r_metrics);
  tokio::spawn(warp::serve(routes).run(([0, 0, 0, 0], port)));

  match thread_handle_task.await {
    Ok(_) => std::process::exit(0),
    Err(e) => {
      error!("Exiting due to error: {:?}", e);
      std::process::exit(1);
    }
  }
}
