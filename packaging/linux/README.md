# Linux packaging

The Linux release target is an AppImage and Flatpak on a validated PipeWire + Wayland screen-portal desktop. PulseAudio and X11 are intentionally not fallback capture paths. The CI workflow performs the CPU/build gates; release qualification must still capture audio and screen on a clean Ubuntu 22.04 machine.
