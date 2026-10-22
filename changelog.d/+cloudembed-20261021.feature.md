The release builds the Open Neural Network Exchange (ONNX) Runtime 1.28.3 from source with telemetry off and attaches it with `packset-embed` linked to it, each archive beside its SHA-256 sum.
`ljos cloud up` installs the pair on a cloud VM only and installs neither when a sum does not match.
