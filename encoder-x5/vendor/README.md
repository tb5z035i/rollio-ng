# Horizon Robotics X5 Multimedia SDK (vendored subset)

## Provenance

These headers are extracted from the Horizon Robotics X5 Linux BSP
`libmultimedia` package. Only the subset required by the rollio encoder
backend is included.

Original location in the BSP sysroot:
```
/usr/include/hb_media_codec.h
/usr/include/hb_media_basic_types.h
/usr/include/hb_media_error.h
/usr/lib/libmultimedia.so.1.2.3
```

## Library

The `lib/` directory contains an unversioned symlink (`libmultimedia.so`)
used only as a link stub at compile time. The actual versioned shared
library (`libmultimedia.so.1`) is loaded at runtime from the target
board's `/usr/lib/`.

## License

The Horizon Robotics Multimedia SDK is proprietary software.
These headers are vendored under the terms of the Horizon Robotics
SDK license agreement for the purpose of cross-compilation only.
