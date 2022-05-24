# drgdfu

[![docs.rs](https://docs.rs/drgdfu/badge.svg)](https://docs.rs/drgdfu)
[![CI](https://github.com/drogue-iot/drgdfu/workflows/CI/badge.svg)](https://github.com/drogue-iot/drgdfu/actions?query=workflow%3A%22CI%22)
[![Matrix](https://img.shields.io/matrix/drogue-iot:matrix.org)](https://matrix.to/#/#drogue-iot:matrix.org)

An firmware update library and tool for devices with DFU capabilities. The devices need to support the serial or GATT based protocols for updating firmware from [Drogue Device](https://github.com/drogue-iot/drogue-device/tree/main/examples/nrf52/microbit/ble).

You can use `drgdfu` as a library in your application (like a BLE gateway), or as a standalone tool.

## Installation

Install using `cargo`:

```
cargo install drgdfu
```

## Supported platforms

* Linux
* Mac OS X
* Windows


## Supported protocols

* Serial
* BLE GATT
* Simulated (for testing)

## Supported firmware sources

* File
* Drogue Cloud running [Drogue Ajour](https://github.com/drogue-iot/drogue-ajour)
