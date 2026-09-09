#!/usr/bin/env python3
"""A tiny but structurally real llama GGUF, for the examples that never read
what the model says.

Two of the sixteen examples -- `bandwidth` and `queueing` -- assert only on
properties of the machinery: how fast weights stream, whether everyone in the
queue was served, whether urgent agents waited less than routine ones. Neither
looks at a single token of output. So neither needs a model that can answer;
it needs a model that can *load and run*, and that is a few hundred kilobytes
rather than 1.2 GB.

`batched` and `generate` are a different matter and this file will not help
them: they check for "Paris" and for the batched replies matching, so they need
weights that were actually trained.

    python3 tools/mk-test-gguf.py /tmp/tiny-llama.gguf
    cargo run --release -p hv2-infer --example queueing -- /tmp/tiny-llama.gguf

The output is deterministic, so two runs give byte-identical files and a
difference in a measurement is never the fixture moving underneath it.
"""

import argparse
import math
import struct
import sys

# --- the shape -------------------------------------------------------------
#
# Small enough to be quick, large enough to be honest: more than one layer so
# the block loop runs more than once, and kv_heads < heads so the grouped-query
# path is exercised rather than skipped.
LAYERS = 2
WIDTH = 64
HEADS = 4
KV_HEADS = 2
HEAD_DIM = WIDTH // HEADS  # 16
KV_DIM = KV_HEADS * HEAD_DIM  # 32
FFN = 128
CONTEXT = 512
RMS_EPS = 1e-5
ROPE_BASE = 10000.0

# GGUF value type tags.
T_UINT32, T_FLOAT32, T_STRING, T_ARRAY = 4, 6, 8, 9
Q_F32 = 0  # the `Quant` discriminant for plain f32

# The specials `chat_turn` and `Model::load` look for by name. The loader takes
# its beginning-of-text marker by name rather than from metadata, so these have
# to be spelled exactly.
SPECIALS = [
    "<|begin_of_text|>",
    "<|end_of_text|>",
    "<|start_header_id|>",
    "<|end_header_id|>",
    "<|eot_id|>",
    "<|eom_id|>",
]


def byte_chars():
    """The byte-to-character table the tokeniser uses, rebuilt here.

    The vocabulary is byte-level: every byte has a printable stand-in, and BPE
    runs over those. So a vocabulary of exactly these 256 characters, with no
    merges at all, encodes any text one token per byte -- which is all these
    two examples need, and it keeps the fixture from pretending to a tokeniser
    it does not have.
    """
    printable = (
        list(range(ord("!"), ord("~") + 1))
        + list(range(0xA1, 0xAD))
        + list(range(0xAE, 0x100))
    )
    table = list(printable)
    spare = 0
    for b in range(256):
        if b not in printable:
            table.append(256 + spare)
            spare += 1
    order = [b for b in range(256) if b in printable] + [
        b for b in range(256) if b not in printable
    ]
    out = [None] * 256
    for byte, code in zip(order, table):
        out[byte] = chr(code)
    return out


def u32(v):
    return struct.pack("<I", v)


def u64(v):
    return struct.pack("<Q", v)


def f32(v):
    return struct.pack("<f", v)


def gstr(s):
    raw = s.encode("utf-8")
    return u64(len(raw)) + raw


def kv_u32(key, value):
    return gstr(key) + u32(T_UINT32) + u32(value)


def kv_f32(key, value):
    return gstr(key) + u32(T_FLOAT32) + f32(value)


def kv_str(key, value):
    return gstr(key) + u32(T_STRING) + gstr(value)


def kv_strs(key, values):
    out = gstr(key) + u32(T_ARRAY) + u32(T_STRING) + u64(len(values))
    return out + b"".join(gstr(v) for v in values)


def weights(count, seed):
    """Deterministic small values.

    Small on purpose: a forward pass over random weights of the wrong scale
    produces infinities, and an example that dies in a softmax would be blamed
    on the scheduler. A plain LCG rather than `random` so the file does not
    depend on a Python version's generator.
    """
    state = seed & 0xFFFFFFFF
    out = bytearray()
    for _ in range(count):
        state = (1664525 * state + 1013904223) & 0xFFFFFFFF
        # (-0.05, 0.05), which keeps activations in a sane range through two
        # layers and leaves the norms well away from zero.
        out += f32(((state / 0xFFFFFFFF) - 0.5) * 0.1)
    return bytes(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("out", help="path to write the .gguf to")
    args = ap.parse_args()

    chars = byte_chars()
    tokens = chars + SPECIALS
    vocab = len(tokens)

    metadata = b"".join(
        [
            kv_str("general.architecture", "llama"),
            kv_u32("llama.block_count", LAYERS),
            kv_u32("llama.embedding_length", WIDTH),
            kv_u32("llama.attention.head_count", HEADS),
            kv_u32("llama.attention.head_count_kv", KV_HEADS),
            kv_u32("llama.feed_forward_length", FFN),
            kv_u32("llama.vocab_size", vocab),
            kv_u32("llama.context_length", CONTEXT),
            kv_f32("llama.attention.layer_norm_rms_epsilon", RMS_EPS),
            kv_f32("llama.rope.freq_base", ROPE_BASE),
            kv_strs("tokenizer.ggml.tokens", tokens),
            # No merges: with a byte-level vocabulary and nothing to merge,
            # every byte is its own token. Valid, and the shortest thing that
            # is.
            kv_strs("tokenizer.ggml.merges", []),
        ]
    )
    kv_count = 12

    # Dimensions are fastest-varying first, so a matrix taking `a` to `b` is
    # [a, b]: `b` rows of `a`. The output head is tied to the embedding, which
    # is why there is no `output.weight` here -- the loader does not look for
    # one.
    plan = [("token_embd.weight", [WIDTH, vocab]), ("output_norm.weight", [WIDTH])]
    for layer in range(LAYERS):
        plan += [
            (f"blk.{layer}.attn_norm.weight", [WIDTH]),
            (f"blk.{layer}.attn_q.weight", [WIDTH, WIDTH]),
            (f"blk.{layer}.attn_k.weight", [WIDTH, KV_DIM]),
            (f"blk.{layer}.attn_v.weight", [WIDTH, KV_DIM]),
            (f"blk.{layer}.attn_output.weight", [WIDTH, WIDTH]),
            (f"blk.{layer}.ffn_norm.weight", [WIDTH]),
            (f"blk.{layer}.ffn_gate.weight", [WIDTH, FFN]),
            (f"blk.{layer}.ffn_up.weight", [WIDTH, FFN]),
            (f"blk.{layer}.ffn_down.weight", [FFN, WIDTH]),
        ]

    blobs, table, offset = [], b"", 0
    for index, (name, dims) in enumerate(plan):
        count = 1
        for d in dims:
            count *= d
        # The norms are multiplied into the residual stream, so they start at
        # one rather than at noise; a norm near zero erases the layer.
        if name.endswith("norm.weight"):
            blob = b"".join(f32(1.0) for _ in range(count))
        else:
            blob = weights(count, seed=1234 + index * 7919)
        table += gstr(name) + u32(len(dims))
        for d in dims:
            table += u64(d)
        table += u32(Q_F32) + u64(offset)
        blobs.append(blob)
        offset += len(blob)

    header = b"GGUF" + u32(3) + u64(len(plan)) + u64(kv_count) + metadata + table
    pad = (-len(header)) % 32  # the default alignment the reader assumes
    body = header + b"\0" * pad + b"".join(blobs)

    with open(args.out, "wb") as fh:
        fh.write(body)

    print(f"wrote {args.out}")
    print(f"  {len(body) / 1024:.0f} KiB, {len(plan)} tensors, vocab {vocab}")
    print(f"  {LAYERS} layers, width {WIDTH}, {HEADS} heads over {KV_HEADS} kv heads")
    print("  it will load and run; it will not say anything sensible")
    return 0


if __name__ == "__main__":
    sys.exit(main())
