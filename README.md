# rustfest-rome18-gtk-gst-workshop

## Packages to download:

### MACOS

```
https://nirbheek.in/files/binaries/gstreamer/1.15.0.1/gtk/macos/gstreamer-1.0-1.15.0.1-x86_64.pkg
https://nirbheek.in/files/binaries/gstreamer/1.15.0.1/gtk/macos/gstreamer-1.0-devel-1.15.0.1-x86_64.pkg
```

### Windows

```
https://nirbheek.in/files/binaries/gstreamer/1.15.0.1/gtk/windows/gstreamer-1.0-windows-msvc-x86_64-1.15.0.1.tar.bz2
```

### Linux

```
apt install libgtk-3-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

## Setup

### On non-Linux platforms

 * Set `PKG_CONFIG_PATH` environment variable to the `lib/pkgconfig` directory
 * Include the bin directory in your `PATH`
 * Set `GST_PLUGIN_SYSTEM_PATH` to `lib/gstreamer-1.0`
 * Set `XDG_DATA_DIRS` to include `share`
 
## Documentation

Docs for GTK+ and GStreamer Rust bindings are available at:

 * https://gtk-rs.org/docs/gtk
 * https://sdroege.github.io/rustdoc/gstreamer/gstreamer
