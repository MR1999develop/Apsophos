Apsophos @VERSION@
================

Photo noise reduction with the SCUNet neural network.
Copyright (C) 2026 MR. Free software,
GPL-3.0-only (LICENSE.txt), with an additional permission for the
NVIDIA and Microsoft libraries it is shipped with (LICENSE-EXCEPTION.txt).

Requirements
------------
- Windows 10 or 11, 64 bit.
- Any graphics card with DirectX 12. On NVIDIA GeForce RTX 30xx and later
  cards the program uses TensorRT for RTX, about 4 times faster; it needs only
  an up-to-date NVIDIA driver. Without a suitable GPU it runs on the CPU,
  much more slowly.

Nothing has to be installed: unzip the folder anywhere and run
apsophos.exe. Keep the DLL files and the "models" folder next to it.

First run
---------
The first time an image is denoised, the program measures which tile size is
fastest on your card (from a few seconds to about a minute with TensorRT for
RTX, which prepares each size once). The result is remembered.

Files the program writes outside its folder
-------------------------------------------
- Settings: %APPDATA%\apsophos (window, folders, parameters)
- GPU engines prepared by TensorRT for RTX:
  %LOCALAPPDATA%\Apsophos\tensorrt-rtx-cache
- Log, one file per day, the last seven kept (Help > Open log folder):
  %LOCALAPPDATA%\Apsophos\logs
Deleting them is harmless: they are recreated.

Exported images never overwrite anything: photo.tif becomes photo_dn.tif,
then photo_dn 2.tif and so on.

Colours on screen
-----------------
Images with their own colour profile (Adobe RGB, ProPhoto...) are shown
converted to sRGB, so they do not look dull. The exported files keep their
own profile.

Judging the result
------------------
N shows, on the right of the comparison, the noise that was removed (x4,
around mid grey): even grain is good; visible edges or texture mean the
settings are taking away detail. The histogram in the right-hand panel
compares the brightness of the original and of the result.

Speed test
----------
"apsophos.exe --self-test report.txt" runs the network on each compute
device without opening the window and writes the timings to report.txt.

If the GPU is not used
----------------------
The Advanced section of the right-hand panel shows the device in use and why
a faster one was skipped; the log has the details.
Some antivirus programs quarantine newly downloaded DLLs: if
tensorrt_rtx_1_4.dll or DirectML.dll disappear from this folder, restore them
from the antivirus history and add an exclusion for the folder.

Source code, licences of the components: see THIRD-PARTY-NOTICES.txt.
