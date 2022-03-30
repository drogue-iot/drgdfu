# drgdfu

An firmware update tool for devices with DFU capabilities. The devices are assumed to support the serial or GATT based protocols for updating firmware from [Drogue Device](https://github.com/drogue-iot/drogue-device/tree/main/examples/nrf52/adafruit-feather-nrf52840/bootloader-dfu).

Supported protocols:

* Serial
* BLE GATT
* Simulated (for testing)

Supported firmware sources:

* File
* Drogue Cloud running [Drogue Ajour](https://github.com/drogue-iot/drogue-ajour)
