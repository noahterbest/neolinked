# Neolinked

![CI](https://github.com/noahterbest/neolinked/workflows/CI/badge.svg)

Neolinked is a small program that acts as a proxy between Reolink IP cameras and
normal RTSP clients.
Certain cameras, such as the Reolink B800, do not implement ONVIF or RTSP, but
instead use a proprietary "Baichuan" protocol only compatible with their apps
and NVRs (any camera that uses "port 9000" will likely be using this protocol).
Neolinked allows you to use NVR software such as Frigate, Shinobi or Blue Iris
to receive video from these cameras instead.
The Reolink NVR is not required, and the cameras are unmodified.
Your NVR software connects to Neolinked, which forwards the video stream from
the camera.

This project is not affiliated with Reolink in any way; everything it does has
been reverse engineered.

## About this fork

Neolinked is a maintained fork of
[QuantumEntangledAndy/neolink](https://github.com/QuantumEntangledAndy/neolink),
which has been dormant since January 2025 with its long-standing resource leaks
unfixed. This fork carries those fixes plus a modernised build.

**The repository, Docker image and project are named `neolinked`. The binary,
the config file format and every command are unchanged (`neolink`), so existing
configs and scripts keep working — you only need to change where you pull the
image from.**

### What's fixed here

- **Runaway memory and file descriptor use.** The RTSP path allocated a
  GStreamer buffer pool for every distinct video frame size and never released
  them, and every client (including monitoring probes) built its own pipeline
  and its own camera session. Together these could consume many gigabytes an
  hour and eventually exhaust file descriptors, which stopped new streams from
  connecting while the process stayed alive.
- **Streams that froze until the container was restarted.** A single
  unrecoverable lost UDP packet could stall a stream permanently while the
  connection still looked healthy. Lost packets are now skipped, the buffers
  behind them are bounded, and a watchdog forces a camera reconnect after 30
  seconds without frames.
- **Unbounded queues when a client stalls.** Stream buffers now drop old frames
  at a fixed cap instead of growing at full video bitrate, which also clears the
  endless `Buffer full on vidsrc` logging.
- **Robustness.** Hardened parsing of camera-supplied lengths, removal of a
  remotely triggerable crash, bounded snapshot and push-notification handling,
  and a number of background tasks that previously leaked per client
  connection.
- **Build and delivery.** CI builds `amd64` and `arm64` and publishes images to
  GitHub Container Registry.

These correspond to upstream issues
[#286](https://github.com/QuantumEntangledAndy/neolink/issues/286),
[#349](https://github.com/QuantumEntangledAndy/neolink/issues/349),
[#366](https://github.com/QuantumEntangledAndy/neolink/issues/366) and
[#370](https://github.com/QuantumEntangledAndy/neolink/issues/370), and include
the approaches from upstream pull requests
[#340](https://github.com/QuantumEntangledAndy/neolink/pull/340)/[#373](https://github.com/QuantumEntangledAndy/neolink/pull/373)
and [#400](https://github.com/QuantumEntangledAndy/neolink/pull/400), none of
which were ever merged upstream.

### Features inherited from upstream

- MQTT
- Motion detection
- Paused streams (when no rtsp client or no motion detected)
- Save a still image to disk
- Multiple ways to reach a camera, including relaying through Reolink servers
- Camera battery levels in the log

## Installation

### Docker (recommended)

Multi-architecture images (`amd64`, `arm64`) are published to GitHub Container
Registry. The `latest` tag tracks `master`.

```bash
docker pull ghcr.io/noahterbest/neolinked:latest
```

```bash
# Add `-e "RUST_LOG=debug"` to run with debug logs
#
# --network host is only needed if you require to connect
# via local broadcasts. If you can connect via any other
# method then normal bridge mode should work fine
# and you can omit this option.
docker run --network host \
  --memory=1g --restart=unless-stopped \
  --volume=$PWD/config.toml:/etc/neolink.toml \
  ghcr.io/noahterbest/neolinked:latest
```

On **Unraid**, set the container's *Repository* field to
`ghcr.io/noahterbest/neolinked:latest`. All other template settings (config
path, ports, network) carry over unchanged.

The `--memory` limit and restart policy above are a safety net, not a
requirement — a healthy install should sit in the tens of megabytes.

#### Environment variables

- `NEO_LINK_MODE`: defaults to `"rtsp"`, other options are `"mqtt"` or
  `"mqtt-rtsp"`.
- `NEO_LINK_PORT`: defaults to `8554`, set this to your required port value.

### From source

Install the latest [gstreamer](https://gstreamer.freedesktop.org/download/),
then `cargo build --release`.

- **Windows**: ensure you install `full` when prompted in the MSI options.
- **Mac**: Install the dpkg version on the official gstreamer website over
  the brew version
- **Ubuntu/Debian**: These packages should work

```bash
sudo apt install \
  libgstrtspserver-1.0-0 \
  libgstreamer1.0-0 \
  libgstreamer-plugins-bad1.0-0 \
  gstreamer1.0-x \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  libssl
```

- **Windows**: You may also need to
  [install openssl](https://wiki.openssl.org/index.php/Binaries)
- **Macos**: You may also need to
  [install openssl](https://wiki.openssl.org/index.php/Binaries) or
  `brew install openssl@1.1`
- **Ubuntu/Debian**: Install the `libssl` package

Make a config file, see below.

## Config/Usage

### RTSP

To use `neolink` you need a config file.

There's a more complete example
[here](https://github.com/noahterbest/neolinked/blob/master/sample_config.toml),
but the following should work as a minimal example.

```toml
bind = "0.0.0.0"

[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"

[[cameras]]
name = "Camera02"
username = "admin"
password = "password"
uid = "BCDEF0123456789A"
address = "192.168.1.10"
```

Create a text file called `neolink.toml` in the same folder as the
neolink binary. With your config options.

When ready start `neolink` with the following command
using the terminal in the same folder the neolink binary is in.

```bash
./neolink rtsp --config=neolink.toml
```

### Recommended settings

- **Give cameras an `address` where you can.** A camera reached by
  `address = "192.168.1.10:9000"` connects over TCP and skips the UDP transport
  entirely, which is the more robust path.
- **Turn off push notifications** with `push_notifications = false` in each
  `[[cameras]]` section. Google removed the API this relied on, so it can no
  longer wake cameras — leaving it enabled only produces retry traffic.
- **Point one consumer at Neolinked** and let it fan out to viewers (Frigate's
  restream or go2rtc, for example). Each direct client still costs a camera
  session, and cameras allow only a few.

### Discovery

To connect to a camera using a UID we need to find the IP address of the camera
with that UID

The IP is discovered with four methods

1. Local discovery: Here we send a broadcast on all visible networks asking
   the local network if there is a camera with this UID. This only works if
   the network supports broadcasts

   If you know the ip address you can put it into the `address` field of the
   config and attempt a direct connection without broadcasts. This requires a
   route from neolink to the camera.

2. Remote discovery: Here we ask the reolink servers what the IP address is.
   This requires that we contact reolink and provide some basic information
   like the UID. Once we have this information we connect directly to the
   local IP address. This requires a route from neolink to the camera and
   for the camera to be able to contact reolink.

3. Map discovery: In this case we register our IP address with reolink and ask
   the camera to connect to us. Once the camera either polls/recieves a connect
   request from the reolink servers the camera will initiate a connection
   to neolink. This requires that our IP and reolink are reachable from
   the camera.

4. Relay: In this case we request that reolink relay our connection. Neither
   neolink nor the camera need to be able to directly contact each other. But
   both neolink and the camera need to be able to contact reolink.

This can be controlled with the config

```toml
discovery = "local"
```

In the `[[cameras]]` section of the toml.

Possible values are `local`, `remote`, `map`, `relay` later values implictly
enable prior methods.

#### Cellular

Cellular cameras should select `"cellular"` which only enables `map` and
`relay` since `local` and `remote` will always fail

```toml
discovery = "cellular"
```

See the sample config file for more details.

### MQTT

To use mqtt you will need to adjust your config file as such:

```toml
bind = "0.0.0.0"

[mqtt]
broker_addr = "127.0.0.1" # Address of the mqtt server
port = 1883 # mqtt servers port
credentials = ["username", "password"] # mqtt server login details

[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"
```

Then to start the mqtt+rtsp connection run the following:

```bash
./neolink mqtt-rtsp --config=neolink.toml
```

OR for only mqtt

```bash
./neolink mqtt --config=neolink.toml
```

Neolink will publish these messages:

Messages that are prefixed with `neolink/`

- `/status` Tracks the connection of neolink, `connected` for ready `offline`
  for not ready this is a LastWill message
- `/config` The configuration file used to start neolink, you can publish to
  this to **temporarily** alter the live configuration
- `/config/status` If you publish to `/config` then any errors from your
  publish config will show here, or `Ok(())` if no errors and finished loading

Messages that are prefixed with `neolink/{CAMERANAME}`

Control messages:

- `/control/led [on|off]` Turns status LED on/off
- `/control/ir [on|off|auto]` Turn IR lights on/off or automatically via light
  detection
- `/control/reboot` Reboot the camera
- `/control/ptz [up|down|left|right|in|out] (amount)` Control the PTZ
  movements, amount defaults to 32.0
- `/control/ptz/preset [id]` Move the camera to a PTZ preset
- `/control/ptz/assign [id] [name]` Set the current PTZ position to a preset ID
  and name
- `/control/zoom (amount)` Zoom the camera to the specified amount. Example: 1.0
  for normal and 3.5 for 3.5x zoom factor. This only works on cameras that support
  zoom
- `/control/pir [on|off]`
- `/control/floodlight [on|off]` Turns floodlight (if equipped) on/off
- `/control/floodlight_tasks [on|off]` Turns floodlight (if equipped) tasks on/off
  This is the automatic tasks such as on motion and night triggers
- `/control/wakeup (mins)` For cameras that are using `idle_disconnect` this will
  force a wakeup for at least the given minutes
- `/control/siren on` Signal the siren, the message is always "on" as there is no
  "off" signal for the siren

Status Messages:

- `/status disconnected` Sent when the camera goes offline
- `/status/battery` Sent in reply to a `/query/battery` an XML encoded version
  of the battery status
- `/status/battery_level` A simple % value of current battery level, only
  published when `enable_battery` is true in the config
- `/status/pir` Sent in reply to a `/query/pir` an XML encoded version of the
  pir status
- `/status/motion` Contains the motion detection alarm status. `on` for motion
  and `off` for still, only published when `enable_moton` is true in the config
- `/status/ptz/preset` Sent in reply to a `/query/ptz/preset` an XML encoded
  version of the PTZ presets
- `/status/preview` a base64 encoded camera image updated every 2s. Not
  every camera supports the snapshot command needed for this. In such cases
  there will be no `/status/preview` message. Only published when
  `enable_preview` is true in the config
- `/status/floodlight_tasks` The current status of the floodlight tasks
  used updated every 2s by default

Query Messages:

- `/query/battery` Request that the camera reports its battery level
- `/query/pir` Request that the camera reports its pir status
- `/query/ptz/preset` Request that the camera reports its PTZ presets
- `/query/preview` Request that the camera post a base64 encoded jpeg
  of the stream to `/status/preview` now, ignoring the timer

### Controlling RTSP from MQTT

If neolink is started with `mqtt-rtsp` then the `/neolink/config` can be used
to control the RTSP

Changes made to the config by publishing to `/neolink/config` should be
reflected in the rtsp

These include changing the:

- Available users

```toml
[[users]]
  name = "me"
  pass = "mepass"
```

- Permitted users on a camera

```toml
[[cameras]]
  permitted_users = [ "me" ]
```

- Available streams

```toml
[[cameras]]
  stream = "Main"
```

Setting a value of `None` will disable the stream

- Disable the entire camera (mqtt updates and all)

```toml
[[cameras]]
  enabled = false
```

### MQTT Disable Features

Certain features like preview and motion detection may not be desired
you can disable them with the following config options.
Disabling these may help to conserve battery

```toml
bind = "0.0.0.0"

[mqtt]
broker_addr = "127.0.0.1" # Address of the mqtt server
port = 1883 # mqtt servers port
credentials = ["username", "password"] # mqtt server login details

[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"
[cameras.mqtt]
enable_motion = false        # motion detection
                             # (limited battery drain since it
                             # is a passive listening connection)
                             #
enable_light = false         # flood lights only available on some camera
                             # (limited battery drain since it
                             # is a passive listening connection)
                             #
enable_battery = false       # battery updates in `/status/battery_level`
                             #
enable_preview = false       # preview image in `/status/preview`
                             #
enable_floodlight = false    # preview image in `/status/floodlight_tasks`
                             #
battery_update = 2000        # Number of ms between `/status/battery_level` updates
                             #
preview_update = 2000        # Number of ms between `/status/preview` updates
                             #
floodlight_update = 2000     # Number of ms between `/status/floodlight_tasks` updates
```

#### MQTT Discovery

[MQTT Discovery](https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery)
is partially supported. Currently, discovery is opt-in and camera features
must be manually specified.

```toml
[cameras.mqtt]
  # <see above>
  [cameras.mqtt.discovery]
  topic = "homeassistant"
  features = ["floodlight"]
```

Available features are:

- `floodlight`: This adds a light control to home assistant
- `camera`: This adds a camera preview to home assistant. It is only updated
  every 0.5s and cannot be much more than that since it is updated over mqtt
  not over RTSP. Not every camera supports the snapshot command needed for
  this. In such cases there will be no `/status/preview` message.
- `led`: This adds a switch to chage the LED status light on/off to home
  assistant
- `ir`: This adds a selection switch to chage the IR light on/off/auto to home
  assistant
- `motion`: This adds a motion detection binary sensor to home assistant
- `reboot`: This adds a reboot button to home assistant
- `pt`: This adds a selection of buttons to control the pan and tilt of the
  camera
- `battery`: This adds a battery level sensor to home assistant
- `siren`: Adds a siren button to home assistant

### Extra Camera Settings

Listed below are extra camera settings:

```toml
[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"
debug = false # Displays Debug XML messages from camera
enabled = true # Enable or Disable the camera
update_time = false # When camera connects, force the setting of the camera date/time to now. The default is false
print_format = "None"  # Type of format that logs are displayed in (None, Human, Xml). The default is None
```

- **Debug:** Will dump the various XMLs from the camera as they are recieved
and decrypted. Leave this off unless asked for it to fix an issue.

- **Enabled:** Useful if you want to remove a camera from rtsp without deleting
it from the config

- **update_time:** Used to FORCE an update on the camera time. Usually it checks
if it is needed but this
will force it regardless. (Mostly this was introduced to address a specific
issue a user had)

- **print_format:** Used for adjusting printing of some values mostly, battery
messages

### Pause

To use the pause feature you will need to adjust your config file as such:

```toml
bind = "0.0.0.0"

[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"
  [cameras.pause]
  on_motion = true # Should pause when no motion
  on_client = true # Should pause when no rtsp client
  timeout = 2.1 # How long to wait after motion stops before pausing
```

Then start the rtsp server as usual:

```bash
./neolink rtsp --config=neolink.toml
```

### Idle Disconnects

To really save battery we need to disconnect the camera when it is idle.

To acheieve this you can add `idle_disconnect = true` to the `[[cameras]]`
section

```toml
bind = "0.0.0.0"

[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"
idle_disconnect = true
[cameras.pause]
  on_client = true # Should pause when no rtsp client
  timeout = 2.1 # How long to wait after motion stops before pausing
```

When `idle_disconnect = true` neolink will disconnect from the camera 30s
after it stops being used.

Neolink considers it as being used if there is an active stream running, or
if there is motion being detected or an mqtt command being run

Once in the disconnected state, neolink will stay disconnected until there is a
new requested activation such as a client connecting or an mqtt command.

You can make neolink stop active streams when there are no rtsp clients using

```toml
[cameras.pause]
  on_client = true # Should pause when no rtsp client
```

> **Note:** Waking on push notifications no longer works — Google removed the
> API it depended on. Motion will not wake a disconnected camera. Set
> `push_notifications = false` in `[[cameras]]` to avoid the pointless retry
> traffic.

### Image

You can write an image from the stream to disk using:

```bash
neolink image --config=config.toml --file-path=filepath CameraName
```

Where filepath is the path to save the image to and CameraName is the name of
the camera from the config to save the image from.

File is always jpeg and the extension given in filepath will be added or changed
to reflect this.

Some cameras do not support the SNAP command that is used to generate the image
on the camera. If this is the case with your camera you can try the
`--use-stream` option which will instead create a jpeg by transcoding the video
stream.

### Battery Levels

You can get the battery level and status using

```bash
neolink battery --config=config.toml CameraName
```

This will produce an xml formatted battery status on stdout for processing

### PIR

You can control pir using

```bash
neolink pir --config=config.toml CameraName [on|off]
```

This will turn the PIR on or off

### Reboot

You can reboot a camera using

```bash
neolink reboot --config=config.toml CameraName
```

### Status LED

You can control the status LED using

```bash
neolink status-light --config=config.toml CameraName [on|off]
```

### Talk

You can talk over the camera using

```bash
neolink talk --config=config.toml --adpcm-file=data.adpc\
               --sample-rate=16000 --block-size=512 CameraName
```

Where the sounds is ADPCM encoded

or

```bash
neolink talk --config=config.toml --microphone  CameraName
```

Which uses the default microphone which depends on
[gstreamer](https://gstreamer.freedesktop.org/documentation/autodetect/autoaudiosrc.html?gi-language=c#autoaudiosrc-page)

### PTZ

You can control the PTZ using

```bash
neolink ptz --config=config.toml CameraName control 32 [left|right|up|down|in|out]
```

Where 32 is the speed. Not all cameras support speed

Some cameras also support preset positions

```bash
# Print the list of preset positions
neolink ptz --config=config.toml CameraName preset
# Move the camera to preset ID 0
neolink ptz --config=config.toml CameraName preset 0
# Save the current position as preset ID 0 with name PresetName
neolink ptz --config=config.toml CameraName assign 0 PresetName
```

To change the zoom level use the following:

```bash
# Zoom the camera to 2.5x
neolink ptz --config=config.toml CameraName zoom 2.5
```

With 1.0 being normal and 2.5 being 2.5x zoom

## Credits

Neolinked stands on the work of others:

- [thirtythreeforty/neolink](https://github.com/thirtythreeforty/neolink) —
  George Hilliard, the original project and Baichuan reverse engineering
- [QuantumEntangledAndy/neolink](https://github.com/QuantumEntangledAndy/neolink)
  — Andrew King, whose fork added MQTT, motion detection, paused streams and
  most of the camera features here

If you find the upstream work helpful, consider supporting its author:

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/G2G5HOYIZ)

## License

Neolinked is free software, released under the GNU Affero General Public
License v3.

This means that if you incorporate it into a piece of software available over
the network, you must offer that software's source code to your users.
