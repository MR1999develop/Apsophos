# Security

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue:
open the repository's **Security** tab and choose **Report a
vulnerability** (GitHub private vulnerability reporting). Only the
maintainer sees the report.

Useful details: the version (Help ▸ About), what you did, what happened,
and, if you can, a file that shows the problem. Images that make the
program crash or misbehave are welcome; they become tests once the
problem is fixed.

You will get an answer as soon as possible. A fix is released first, and
the problem is described publicly afterwards, with credit to you if you
wish.

## Scope

Apsophos is a desktop program that opens image files (TIFF, JPEG, PNG)
and runs a neural network on them, without a network connection. The
most relevant problems are those an image file can cause: crashes,
excessive memory or time, files written where they should not be, or
metadata read wrongly. Problems in third-party components (ONNX Runtime,
DirectML, TensorRT for RTX, egui) are best reported to them too.

Only the latest release is supported.
