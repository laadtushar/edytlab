#!/usr/bin/env python3
"""Regenerate `identity.onnx`, the model `tests/cache_smoke.rs` loads.

The model is one node, `Y = Identity(X)`, with `X` and `Y` both float32 of
shape [1, 3] (ONNX IR version 8, default-domain opset 13). It is 91 bytes.
It exists so CI can build a real ONNX Runtime session from a real model
file and check that what comes out is what went in, against the runtime
version pinned in `apps/desktop/src-tauri/ml-runtime.json`, without
downloading a speech model.

The file is encoded by hand, as protobuf, so this needs only Python 3: no
`onnx` package, no PyTorch. The field numbers are the ones in
https://github.com/onnx/onnx/blob/main/onnx/onnx.proto (ModelProto,
GraphProto, NodeProto, ValueInfoProto, TypeProto, TensorShapeProto,
OperatorSetIdProto). The output is deterministic, so regenerating gives the
committed bytes exactly; if it does not, this script or the file was edited
and one of them is wrong.

    python3 crates/ml-pipeline/tests/fixtures/make_identity_onnx.py

writes `identity.onnx` next to this script. Pass a path to write somewhere
else, e.g. to compare with `cmp` before committing.
"""
import os
import sys


def varint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        out.append(b | (0x80 if n else 0))
        if not n:
            return bytes(out)


def key(field, wire_type):
    return varint((field << 3) | wire_type)


def length_delimited(field, payload):
    return key(field, 2) + varint(len(payload)) + payload


def int_field(field, n):
    return key(field, 0) + varint(n)


def string_field(field, text):
    return length_delimited(field, text.encode())


def dim(value):
    # TensorShapeProto.dim { Dimension.dim_value }
    return length_delimited(1, int_field(1, value))


FLOAT = 1  # TensorProto.DataType.FLOAT


def value_info(name):
    # ValueInfoProto { name, type { tensor_type { elem_type, shape [1, 3] } } }
    shape = length_delimited(2, dim(1) + dim(3))
    tensor_type = length_delimited(1, int_field(1, FLOAT) + shape)
    return string_field(1, name) + length_delimited(2, tensor_type)


# NodeProto { input "X", output "Y", op_type "Identity" }
node = string_field(1, "X") + string_field(2, "Y") + string_field(4, "Identity")

# GraphProto { node, name, input, output }
graph = (
    length_delimited(1, node)
    + string_field(2, "g")
    + length_delimited(11, value_info("X"))
    + length_delimited(12, value_info("Y"))
)

# ModelProto { ir_version 8, producer_name, graph, opset_import { domain "", version 13 } }
model = (
    int_field(1, 8)
    + string_field(2, "edytlab-fixtures")
    + length_delimited(7, graph)
    + length_delimited(8, string_field(1, "") + int_field(2, 13))
)

if __name__ == "__main__":
    out = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.join(os.path.dirname(os.path.abspath(__file__)), "identity.onnx")
    )
    with open(out, "wb") as f:
        f.write(model)
    print("%s: %d bytes" % (out, len(model)))
