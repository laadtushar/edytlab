#!/usr/bin/env python3
"""Make `htdemucs_split/htdemucs_fwd.onnx` and `htdemucs_fwd.onnx.data`.

A model with the pinned htdemucs graph's inputs and outputs (names, shapes,
types: see `src/htdemucs.rs`) and none of its network, so that the pipeline
can be run on the real ONNX Runtime in a few seconds and a few KB:

    x     [1, 4, 2048, 336]  ->  x_out  [1, 16, 2048, 336]
    xt    [1, 2, 343980]     ->  xt_out [1, 8, 343980]

Each output is its input four times over (one copy per source, along the
channel axis, in the layout of the real one: source-major) times 0.125. The
network around it normalises the inputs and undoes that on the outputs, and
adds the two branches, so each of the four sources comes out as a quarter of
the input (0.125 from the time branch, 0.125 from the frequency branch) and
the four add up to it. That is a test of the layout of the tensors, the
order of the planes and the arithmetic around the graph, on the real
runtime. It says nothing about separation.

The 0.125 is an initializer stored in an external data file named
`htdemucs_fwd.onnx.data`, as the real export stores its weights, so that the
two-file layout (and the folder the loader checks for both) is the real one.

Needs `onnx`: pip install onnx. Run from anywhere:

    python3 make_split_model.py
"""
import os

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper
from onnx.external_data_helper import convert_model_to_external_data

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "htdemucs_split")
SOURCES = 4

x = helper.make_tensor_value_info("x", TensorProto.FLOAT, [1, 4, 2048, 336])
xt = helper.make_tensor_value_info("xt", TensorProto.FLOAT, [1, 2, 343980])
x_out = helper.make_tensor_value_info("x_out", TensorProto.FLOAT, [1, 4 * SOURCES, 2048, 336])
xt_out = helper.make_tensor_value_info("xt_out", TensorProto.FLOAT, [1, 2 * SOURCES, 343980])

fraction = numpy_helper.from_array(np.array(0.125, dtype=np.float32), name="fraction")

nodes = [
    helper.make_node("Concat", ["x"] * SOURCES, ["x_copies"], axis=1),
    helper.make_node("Mul", ["x_copies", "fraction"], ["x_out"]),
    helper.make_node("Concat", ["xt"] * SOURCES, ["xt_copies"], axis=1),
    helper.make_node("Mul", ["xt_copies", "fraction"], ["xt_out"]),
]
graph = helper.make_graph(nodes, "htdemucs_split", [x, xt], [x_out, xt_out], [fraction])
model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 20)])
model.ir_version = 10
onnx.checker.check_model(model)

os.makedirs(OUT, exist_ok=True)
convert_model_to_external_data(
    model, all_tensors_to_one_file=True, location="htdemucs_fwd.onnx.data", size_threshold=0
)
onnx.save_model(model, os.path.join(OUT, "htdemucs_fwd.onnx"))
print("wrote", OUT)
