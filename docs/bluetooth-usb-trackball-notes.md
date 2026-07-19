# Kensington trackball Bluetooth/USB switching notes

This note records the July 14, 2026 investigation for a Kensington-style
trackball that would not appear during Bluetooth pairing. Use it as context
when adding a popmgr feature for switching a pointing device between USB,
2.4 GHz receiver, and Bluetooth modes.

## Current findings

- The host Bluetooth controller was healthy:
  - `bluetoothctl show`: `Powered: yes`, `Pairable: yes`
  - `rfkill list bluetooth`: not soft-blocked or hard-blocked
  - `systemctl status bluetooth`: `bluetooth.service` active
- Bluetooth scanning worked. Other nearby devices appeared, including known
  phones, laptops, and a `Logitech Pebble`.
- No scanned device advertised as `Kensington`, `Trackball`, or an obvious HID
  mouse during the session.
- The only paired Bluetooth device was `CW1`, which identified as a phone-like
  device, not the trackball.
- `lsusb` did not show a Kensington USB receiver or direct Kensington USB
  device at the time of checking.
- `/proc/bus/input/devices` did not show a Kensington USB mouse/trackball. The
  visible USB input device was `ILITEK ILITEK-TP` touch input.

Interpretation: the PC-side Bluetooth stack was not the blocker. The trackball
was likely in USB or 2.4 GHz mode, already attached elsewhere, or not in active
Bluetooth pairing mode.

## Manual recovery procedure

When testing at home, start with the physical device state:

1. Disconnect the USB cable or 2.4 GHz receiver if present.
2. Put the trackball in Bluetooth mode, not USB or receiver mode.
3. Hold the Bluetooth channel/pairing button until the LED blinks quickly.
4. Run a short scan:

```bash
bluetoothctl show
rfkill list bluetooth
bluetoothctl --timeout 20 scan on
bluetoothctl devices
```

5. If the device appears, inspect it:

```bash
bluetoothctl info <MAC>
```

6. Pair, trust, and connect:

```bash
bluetoothctl pair <MAC>
bluetoothctl trust <MAC>
bluetoothctl connect <MAC>
```

7. Confirm input registration:

```bash
bluetoothctl devices Connected
cat /proc/bus/input/devices
```

## USB-side disconnect options

Only disconnect a USB device from software after identifying the exact sysfs
path. Do not unbind a guessed path.

Useful discovery commands:

```bash
lsusb
cat /proc/bus/input/devices
find /sys/bus/usb/devices -maxdepth 2 -type f -name product -print
find /sys/bus/usb/devices -maxdepth 2 -type f -name manufacturer -print
```

Safer per-device re-enumeration uses the device `authorized` file:

```bash
echo 0 | sudo tee /sys/bus/usb/devices/<USB_SYSFS_NAME>/authorized
sleep 0.3
echo 1 | sudo tee /sys/bus/usb/devices/<USB_SYSFS_NAME>/authorized
```

Driver unbind is stronger and easier to misuse:

```bash
echo '<USB_SYSFS_NAME>' | sudo tee /sys/bus/usb/drivers/usb/unbind
```

Use unbind only when the target is confirmed, because it can detach a hub,
touchscreen, keyboard, or unrelated device.

## Feature ideas for popmgr

- Add a Bluetooth/USB switching card under the USB tab or a new input-devices
  section.
- Show three states separately:
  - USB receiver/direct USB present
  - Bluetooth controller health
  - Bluetooth paired/connected HID devices
- Detect candidate HID devices by:
  - USB VID/PID and product/manufacturer strings
  - `/proc/bus/input/devices` names
  - Bluetooth UUID `00001812-0000-1000-8000-00805f9b34fb` for HID over GATT
  - device names containing `Kensington`, `Trackball`, `Mouse`, or known model
    strings
- Prefer `authorized` toggling for USB re-enumeration before offering driver
  unbind.
- Require a confirmation dialog before disconnecting USB hubs or devices that
  provide keyboard/touch input.
- For Bluetooth pairing, run a timed scan and display only likely HID
  candidates, plus an "advanced: show all" list.
- If no Kensington/HID candidate appears, tell the user to switch the physical
  device to Bluetooth mode and hold the pairing button until the LED blinks
  quickly.

## Known command caveats

- `bluetoothctl paired-devices` is not available on this system. Use:

```bash
bluetoothctl devices Paired
```

- `bluetoothctl scan off` may print `Failed to stop discovery:
  org.bluez.Error.Failed` even after the scan timeout has ended. Verify with:

```bash
bluetoothctl show
```

and check `Discovering: no`.

- Piping or repeatedly spawning `bluetoothctl` from shell loops produced D-Bus
  assertion failures in this environment. Prefer one `bluetoothctl` command per
  action, or use a proper BlueZ D-Bus client in application code.
