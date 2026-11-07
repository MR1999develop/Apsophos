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

4. Upscaling models (optional). Users download them from inside the program
   (a button; the list with sizes and SHA-256 is upscalers.txt, published by
   .github/workflows/models.yml in the release models-1). To make them here,
   from the SwinIR real-world weights (Apache-2.0), release v0.0 of
   https://github.com/JingyunLiang/SwinIR:
     003_realSR_BSRGAN_DFO_s64w8_SwinIR-M_x2_PSNR.pth      2x, faithful
     003_realSR_BSRGAN_DFO_s64w8_SwinIR-M_x2_GAN.pth       2x, generated detail
     003_realSR_BSRGAN_DFOWMFC_s64w8_SwinIR-L_x4_PSNR.pth  4x, faithful
     003_realSR_BSRGAN_DFOWMFC_s64w8_SwinIR-L_x4_GAN.pth   4x, generated detail
   For each file:
     python tools/export_upscaler_model.py --weights <file.pth> --output-dir models --verify
     python tools/make_fp16_model.py "models/swinir_real_x*.onnx"
   The converter knows each file by its SHA-256 and refuses any other.
