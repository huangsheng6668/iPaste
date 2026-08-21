r"""Verify the exported ONNX graphs against the torch model on a synthetic
Japanese-text image, greedy decoding on both sides, plus the official
beam-search reference.

  python scripts/ocr-models/verify-mocr-onnx.py --src <manga_ocr dir> --onnx <export dir>
"""
import argparse
import pathlib
import time

import numpy as np
import torch
from PIL import Image, ImageDraw, ImageFont

from transformers import AutoTokenizer, VisionEncoderDecoderModel

DECODER_START_TOKEN_ID = 2
EOS_TOKEN_ID = 3
MAX_LENGTH = 300

FONT_CANDIDATES = [
    r"C:\Windows\Fonts\yugothic.ttc",
    r"C:\Windows\Fonts\msgothic.ttc",
    r"C:\Windows\Fonts\meiryo.ttc",
]


def make_image():
    img = Image.new("RGB", (448, 128), "white")
    draw = ImageDraw.Draw(img)
    for font_path in FONT_CANDIDATES:
        try:
            font = ImageFont.truetype(font_path, 36)
            break
        except OSError:
            continue
    else:
        raise RuntimeError("no Japanese font found")
    draw.text((16, 40), "そういえば、昨日の漫画を読んだ", fill="black", font=font)
    return img


def preprocess(img):
    img = img.resize((224, 224), Image.BILINEAR)
    arr = np.asarray(img, dtype=np.float32) / 255.0
    arr = (arr - 0.5) / 0.5
    arr = arr.transpose(2, 0, 1)[None]  # 1,3,224,224
    return np.ascontiguousarray(arr)


def torch_reference(model, img):
    pixel_values = torch.from_numpy(preprocess(img))
    with torch.no_grad():
        ids = model.generate(pixel_values, max_length=MAX_LENGTH)[0].tolist()
    return ids


def onnx_greedy(enc_session, dec_session, img):
    hidden = enc_session.run(None, {"pixel_values": preprocess(img)})[0]
    tokens = [DECODER_START_TOKEN_ID]
    for _ in range(MAX_LENGTH - 1):
        logits = dec_session.run(
            None,
            {
                "input_ids": np.asarray([tokens], dtype=np.int64),
                "encoder_hidden_states": hidden,
            },
        )[0]
        next_token = int(np.argmax(logits[0, -1]))
        tokens.append(next_token)
        if next_token == EOS_TOKEN_ID:
            break
    return tokens


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True)
    ap.add_argument("--onnx", required=True)
    ap.add_argument("--image", default=None)
    args = ap.parse_args()

    import onnxruntime as ort

    providers = ["CPUExecutionProvider"]
    enc = ort.InferenceSession(str(pathlib.Path(args.onnx) / "encoder.onnx"), providers=providers)
    dec = ort.InferenceSession(str(pathlib.Path(args.onnx) / "decoder.onnx"), providers=providers)

    model = VisionEncoderDecoderModel.from_pretrained(args.src)
    model.eval()
    tokenizer = AutoTokenizer.from_pretrained(args.src)

    img = Image.open(args.image) if args.image else make_image()
    img.convert("RGB").save("ocr-spike/mocr-onnx/verify-input.png")

    t0 = time.perf_counter()
    onnx_ids = onnx_greedy(enc, dec, img)
    t_onnx = time.perf_counter() - t0

    t0 = time.perf_counter()
    torch_ids = torch_reference(model, img)
    t_torch = time.perf_counter() - t0

    def decode(ids):
        return tokenizer.decode(
            [i for i in ids if i not in (DECODER_START_TOKEN_ID, EOS_TOKEN_ID)],
            skip_special_tokens=True,
        ).replace(" ", "")

    print(f"torch (beam=4):  [{t_torch:.2f}s] {decode(torch_ids)!r}")
    print(f"onnx  (greedy):  [{t_onnx:.2f}s] {decode(onnx_ids)!r}")
    print("MATCH" if decode(torch_ids) == decode(onnx_ids) else "DIFFER")


if __name__ == "__main__":
    main()
