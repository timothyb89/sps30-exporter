pub fn install_tracing() {
  use tracing_error::ErrorLayer;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::{fmt, EnvFilter};

  let fmt_layer = fmt::layer().with_target(true).with_writer(std::io::stderr);

  let filter_layer = EnvFilter::try_from_default_env()
    .or_else(|_| EnvFilter::try_new("info"))
    .unwrap();

  tracing_subscriber::registry()
    .with(filter_layer)
    .with(fmt_layer)
    .with(ErrorLayer::default())
    .init();
}
