Put the SCUNet models here. They are generated, not stored in the repository.

1. The normal model (required, about 77 MB), from the original KAIR weights:
     python tools/export_onnx_model.py --weights scunet_color_real_psnr.pth \
         --output models/scunet_color_real_448.onnx
   Only Python and NumPy are needed.
   Weights: https://github.com/cszn/KAIR/releases/download/v1.0/scunet_color_real_psnr.pth

2. Larger tiles (optional, faster on a GPU): the same command with
     --tile 704 / 960 / 1216 / 1600 --output models/scunet_color_real_<tile>.onnx

3. Half-precision variants for TensorRT for RTX (optional, about 4 times faster
   on RTX cards); scripts\prepare.bat makes them by itself when Python is available:
     python tools/make_fp16_model.py models/scunet_color_real_*.onnx

