r"""Export manga-ocr-base to split ONNX graphs (encoder / decoder) + fp16.

Run with the local manga-env python:
  python scripts/ocr-models/export-mocr-onnx.py --src <manga_ocr weights dir> --out <staging dir>

Products (written under --out):
  encoder.onnx   pixel_values[1,3,224,224] -> last_hidden_state[1,197,768]   (fp16)
  decoder.onnx   input_ids[1,L] + encoder_hidden_states[1,197,768] -> logits[1,L,6144]  (fp16)
  export-meta.json  sizes/sha256 of the products (for staging manifest upkeep)

The decoder is exported WITHOUT kv-cache (2-layer decoder, full-sequence rerun
per step is cheap) and with dynamic sequence length. Generation params pinned
from the model config: decoder_start_token_id=2, eos=3, max_length=300.
"""
import argparse
import hashlib
import json
import pathlib

import torch
from transformers import AutoTokenizer, VisionEncoderDecoderModel

DECODER_START_TOKEN_ID = 2
EOS_TOKEN_ID = 3


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


class EncoderWrapper(torch.nn.Module):
    def __init__(self, encoder):
        super().__init__()
        self.encoder = encoder

    def forward(self, pixel_values):
        return self.encoder(pixel_values).last_hidden_state


class DecoderWrapper(torch.nn.Module):
    def __init__(self, decoder):
        super().__init__()
        self.decoder = decoder

    def forward(self, input_ids, encoder_hidden_states):
        return self.decoder(
            input_ids=input_ids,
            encoder_hidden_states=encoder_hidden_states,
            use_cache=False,
        ).logits


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--fp16", action="store_true", default=False,
                    help="fp16 conversion is OFF by default: the CPU EP casts fp16 back "
                         "to fp32 per-op (slower), and the decoder graph has Cast-node "
                         "type conflicts under onnxconverter-common. fp32 products are "
                         "larger but served via the Release url-override channel.")
    args = ap.parse_args()

    out_dir = pathlib.Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    model = VisionEncoderDecoderModel.from_pretrained(args.src)
    model.eval()
    tokenizer = AutoTokenizer.from_pretrained(args.src)

    # tokenizer sanity: vocab must be char-level so Rust side can map ids by line
    vocab = tokenizer.convert_ids_to_tokens(list(range(tokenizer.vocab_size)))
    print(f"vocab_size={tokenizer.vocab_size} first8={vocab[:8]}")

    with torch.no_grad():
        pixel_values = torch.randn(1, 3, 224, 224)
        hidden = EncoderWrapper(model.encoder)(pixel_values)
        assert tuple(hidden.shape) == (1, 197, 768), hidden.shape

        input_ids = torch.tensor([[DECODER_START_TOKEN_ID]])
        logits = DecoderWrapper(model.decoder)(input_ids, hidden)
        assert tuple(logits.shape) == (1, 1, tokenizer.vocab_size), logits.shape

        enc_path = out_dir / "encoder.onnx"
        dec_path = out_dir / "decoder.onnx"
        torch.onnx.export(
            EncoderWrapper(model.encoder),
            (pixel_values,),
            str(enc_path),
            input_names=["pixel_values"],
            output_names=["last_hidden_state"],
            opset_version=17,
            dynamo=False,
        )
        torch.onnx.export(
            DecoderWrapper(model.decoder),
            (input_ids, hidden),
            str(dec_path),
            input_names=["input_ids", "encoder_hidden_states"],
            output_names=["logits"],
            dynamic_axes={
                "input_ids": {0: "batch", 1: "seq"},
                "encoder_hidden_states": {0: "batch", 1: "enc_seq"},
                "logits": {0: "batch", 1: "seq"},
            },
            opset_version=17,
            dynamo=False,
        )

    if args.fp16:
        from onnxconverter_common import float16

        for path in (enc_path, dec_path):
            m = onnx_load(path)
            m = float16.convert_float_to_float16(m, keep_io_types=True)
            onnx_save(m, path)

    # vocab.txt：sidecar 解码（id → 文本）需要；从源目录原样拷贝
    import shutil

    vocab_path = out_dir / "vocab.txt"
    shutil.copyfile(pathlib.Path(args.src) / "vocab.txt", vocab_path)

    meta = {
        "encoder": {"file": "encoder.onnx", "bytes": enc_path.stat().st_size, "sha256": sha256(enc_path)},
        "decoder": {"file": "decoder.onnx", "bytes": dec_path.stat().st_size, "sha256": sha256(dec_path)},
        "vocab": {"file": "vocab.txt", "bytes": vocab_path.stat().st_size, "sha256": sha256(vocab_path)},
    }
    (out_dir / "export-meta.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(meta, indent=2))


def onnx_load(path):
    import onnx

    return onnx.load(str(path))


def onnx_save(model, path):
    import onnx

    onnx.save(model, str(path))


if __name__ == "__main__":
    main()
