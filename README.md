# sps30-exporter

A Prometheus (and JSON) exporter for SPS30 sensors. Written in Rust using
the [`sps30` crate][sps30], targeting Linux systems including the Raspberry Pi.

[sps30]: https://github.com/iohe/sps30

## Usage

```bash
$ sps30-exporter /dev/ttyUSB0
```

Then query either the `/metrics` or `/json` endpoints to fetch the latest sensor
reading in the relevant format:

```
$ http get localhost:8090/metrics
HTTP/1.1 200 OK
content-length: 775
content-type: text/plain; charset=utf-8
date: Sun, 13 Mar 2022 02:46:17 GMT

sps30_mass_concentration{variant="PM1.0",unit="μg/m3"} 0.4468425512313843
sps30_mass_concentration{variant="PM2.5",unit="μg/m3"} 0.6282436847686768
sps30_mass_concentration{variant="PM4",unit="μg/m3"} 0.7524338960647583
sps30_mass_concentration{variant="PM10",unit="μg/m3"} 0.8207473754882813
sps30_number_concentration{variant="PM0.5",unit="1/cm3"} 2.692044258117676
sps30_number_concentration{variant="PM1.0",unit="1/cm3"} 3.380452871322632
sps30_number_concentration{variant="PM2.5",unit="1/cm3"} 3.5506482124328613
sps30_number_concentration{variant="PM4",unit="1/cm3"} 3.581928253173828
sps30_number_concentration{variant="PM10",unit="1/cm3"} 3.58970308303833
sps30_typical_particle_size{unit="μm"} 0.710545539855957
sps30_error_count 0
sps30_fatal_error_count 0

$ http get localhost:8090/json
HTTP/1.1 200 OK
content-length: 204
content-type: application/json
date: Sun, 13 Mar 2022 02:46:57 GMT

{
    "mass": {
        "pm1": 0.82750446,
        "pm10": 1.5371889,
        "pm25": 1.1711552,
        "pm4": 1.4072949
    },
    "number": {
        "pm05": 4.9664965,
        "pm1": 6.2515726,
        "pm10": 6.64883,
        "pm25": 6.5746503,
        "pm4": 6.6340795
    },
    "typical_particle_size": 0.71521413
}
```

## Installation (Raspberry Pi)

1. Use `Dockerfile.gnueabihf` to build 32-bit ARM binaries for targets with
    hardware floating point (at least the Pi 2/3/4, and some Zero Ws depending
    on software).

    ```bash
    docker build . -f Dockerfile.gnueabihf -t sps30-exporter:build
    ```

 2. Extract the binaries:

    ```bash
    mkdir -p /tmp/sps30-exporter
    docker run \
      --rm \
      -v /tmp/sps30-exporter:/tmp/sps30-exporter \
      sps30-exporter:build \
      sh -c 'cp /project/target/arm-unknown-linux-gnu*/release/sps30-exporter /tmp/sps30-exporter/'
    ```

 3. Copy the binary `sps30-exporter` from your local `/tmp/sps30-exporter` to
    your Pi's `/usr/local/bin/`.

 4. Copy [`sps30-exporter.service`] to `/etc/systemd/system/` on your Pi.

    Make sure to replace `<DEVICE>` in the `ExecStart=` section with your serial
    port device, e.g. `/dev/ttyUSB0`.

 5. Add the `pi` user to the dialout group:

    ```bash
    usermod -a -G dialout pi
    ```

 6. Enable and start the exporter:
    ```bash
    sudo systemctl enable sps30-exporter
    sudo systemctl start sps30-exporter
    ```

[`sps30-exporter.service`]: ./sps30-exporter.service
