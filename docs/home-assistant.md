# Home Assistant

Nodestarr publishes your containers to Home Assistant over MQTT, using Home
Assistant's MQTT discovery, so they appear as devices without any YAML.

## Set it up

You need an MQTT broker that Home Assistant uses (for example the Mosquitto
add-on or a Mosquitto container) and the MQTT integration in Home
Assistant.

In Settings > Home Assistant:

1. Turn on **Publish to an MQTT broker**, and enter the broker's address,
   port (1883, or 8883 with TLS) and login.
2. Click **Test connection**, then save. The status shows whether Nodestarr
   is connected.
3. Optionally change the topics: discovery prefix `homeassistant` (Home
   Assistant's default) and base topic `nodestarr`.

Within a few seconds, Home Assistant lists a device for Nodestarr and one
for every container (hidden containers are left out).

## Entities

Each container device has:

| Entity | Kind | Notes |
|---|---|---|
| Status | sensor | `running`, `exited`, `paused`, ... with the health, version and usage as attributes. |
| Version | sensor | The app version Nodestarr detected. |
| Image | update | Shows the installed and latest version, so container updates appear in Home Assistant's Updates panel. |
| Running | switch | Starts and stops the container. Only with controls on (below). |
| Restart | button | Only with controls on. |
| CPU, Memory | sensors | Only with the CPU and memory sensors on. |

The update entity can be installed from Home Assistant when controls are on;
that runs the same update as the Update button in Nodestarr (backup,
verification, rollback).

The Nodestarr device has **Updates available** and **Containers running**
sensors and a **Check for updates** button.

## Options

- **Buttons to start, stop, restart and update containers** (on by
  default): the switches, restart buttons and installable updates. Anyone
  who can use your Home Assistant can then control containers. With this
  off, Home Assistant can watch but not act: Nodestarr doesn't listen for
  commands and the update entities are read-only. Nodestarr's own container
  never gets controls.
- **CPU and memory sensors for each container** (on by default), updated at
  the resource sampling interval.

Actions taken from Home Assistant appear in Nodestarr's activity log as done
by Home Assistant.

## Topics

For automations or other MQTT clients:

| Topic | Content |
|---|---|
| `nodestarr/status` | `online` or `offline` (retained; `offline` is also the last will) |
| `nodestarr/summary` | `{"running": 14, "updates": 2}` |
| `nodestarr/<container>/state` | `{"state": "running", "health": null, "running": true, "version": "5.0.0.2569", "cpu": 0.4, "memory_mb": 182}` |
| `nodestarr/<container>/update` | Home Assistant update entity state |
| `nodestarr/<container>/set` | Commands: `start`, `stop`, `restart`, `update` (with controls on) |
| `nodestarr/command` | `check_updates` |

`<container>` is the container's identity in lowercase with dashes, for
example `compose-media-sonarr` for the `sonarr` service of the `media`
stack.

Nodestarr republishes everything when Home Assistant restarts (it watches
`homeassistant/status`) and removes the entities of containers that are
gone.

## Prometheus

Nodestarr also serves Prometheus metrics; see
[Monitoring](monitoring.md#prometheus).
