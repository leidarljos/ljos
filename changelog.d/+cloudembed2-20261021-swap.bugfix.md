A failed encoder install no longer leaves the new ONNX Runtime in place and drops the previous one.
`ljos cloud up` stages the runtime and `packset-embed`, swaps them together, and puts the previous pair back if either rename fails.
