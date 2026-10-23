// Hooks for the runtime's `readback.rs` (see ../hooks.rs).
&[
    // Any resource, at any time: its destructors free its CPU memory before
    // waiting for a readback into it; the host waits first (readback.rs).
    // The wait, then each resource type's destructor and deleting destructor.
    ("wait_readback", "wasm_null_d3d::HasGpuId::WaitReadback()"),
    ("buffer_dtor", "wasm_null_d3d::NullResource<wasm_null_d3d::Stub_ID3D11Buffer, D3D11_BUFFER_DESC>::~NullResource()"),
    ("buffer_delete", "wasm_null_d3d::NullResource<wasm_null_d3d::Stub_ID3D11Buffer, D3D11_BUFFER_DESC>::~NullResource()_567"),
    ("texture1d_dtor", "wasm_null_d3d::NullResource<wasm_null_d3d::Stub_ID3D11Texture1D, D3D11_TEXTURE1D_DESC>::~NullResource()"),
    ("texture1d_delete", "wasm_null_d3d::NullResource<wasm_null_d3d::Stub_ID3D11Texture1D, D3D11_TEXTURE1D_DESC>::~NullResource()_586"),
    ("texture2d_dtor", "wasm_null_d3d::NullResource<wasm_null_d3d::Stub_ID3D11Texture2D, D3D11_TEXTURE2D_DESC>::~NullResource()"),
    ("texture2d_delete", "wasm_null_d3d::NullResource<wasm_null_d3d::Stub_ID3D11Texture2D, D3D11_TEXTURE2D_DESC>::~NullResource()_472"),
    ("texture3d_dtor", "wasm_null_d3d::NullResource<wasm_null_d3d::Stub_ID3D11Texture3D, D3D11_TEXTURE3D_DESC>::~NullResource()"),
    ("texture3d_delete", "wasm_null_d3d::NullResource<wasm_null_d3d::Stub_ID3D11Texture3D, D3D11_TEXTURE3D_DESC>::~NullResource()_605"),
]
