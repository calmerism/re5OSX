//! What the build needs to know about game.wasm: imports, exports, function
//! names and signatures.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use wasmparser::{ExternalKind, KnownCustom, Name, Parser, Payload, TypeRef, ValType};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    I32,
    I64,
    F32,
    F64,
    /// Only used internally by the module; never crosses the host boundary.
    V128,
}

#[derive(Clone, Debug)]
pub struct Sig {
    pub params: Vec<Ty>,
    pub result: Option<Ty>,
}

impl Sig {
    pub fn crosses_boundary_ok(&self) -> bool {
        !self.params.contains(&Ty::V128) && self.result != Some(Ty::V128)
    }
}

pub struct Import {
    pub module: String,
    pub name: String,
    pub sig: Sig,
}

pub struct Export {
    pub name: String,
    pub sig: Sig,
}

pub struct Memory {
    pub initial_pages: u64,
    pub max_pages: u64,
}

pub struct Module {
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub memory: Memory,
    /// Function name (name section) -> signature, for defined functions.
    pub functions: HashMap<String, Sig>,
}

pub fn parse(wasm: &[u8]) -> Result<Module> {
    let mut types = Vec::new();
    let mut imports = Vec::new();
    let mut memory = None;
    let mut func_types: Vec<u32> = Vec::new(); // whole function index space
    let mut imported_funcs = 0;
    let mut exports = Vec::new();
    let mut names = HashMap::new();

    for payload in Parser::new(0).parse_all(wasm) {
        match payload? {
            Payload::TypeSection(reader) => {
                for ty in reader.into_iter_err_on_gc_types() {
                    let ty = ty?;
                    let params = ty.params().iter().map(conv).collect::<Result<_>>()?;
                    let result = match ty.results() {
                        [] => None,
                        [t] => Some(conv(t)?),
                        _ => bail!("multi-value results are not supported"),
                    };
                    types.push(Sig { params, result });
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import?;
                    match import.ty {
                        TypeRef::Func(ty) => {
                            func_types.push(ty);
                            imported_funcs += 1;
                            imports.push((import.module.to_string(), import.name.to_string(), ty));
                        }
                        TypeRef::Memory(m) if import.module == "env" && import.name == "memory" => {
                            if !(m.memory64 && m.shared) {
                                bail!("expected a shared 64-bit memory");
                            }
                            memory = Some(Memory {
                                initial_pages: m.initial,
                                max_pages: m.maximum.context("shared memory without maximum")?,
                            });
                        }
                        other => bail!("unexpected import {}.{}: {other:?}", import.module, import.name),
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for ty in reader {
                    func_types.push(ty?);
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    if export.kind == ExternalKind::Func {
                        exports.push((export.name.to_string(), export.index));
                    }
                }
            }
            Payload::CustomSection(section) => {
                if let KnownCustom::Name(reader) = section.as_known() {
                    for name in reader {
                        if let Name::Function(map) = name? {
                            for naming in map {
                                let naming = naming?;
                                names.insert(naming.name.to_string(), naming.index);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let sig = |ty: u32| types.get(ty as usize).cloned().context("bad type index");
    Ok(Module {
        imports: imports
            .into_iter()
            .map(|(module, name, ty)| Ok(Import { module, name, sig: sig(ty)? }))
            .collect::<Result<_>>()?,
        exports: exports
            .into_iter()
            .map(|(name, index)| Ok(Export { name, sig: sig(func_types[index as usize])? }))
            .collect::<Result<_>>()?,
        memory: memory.context("game.wasm does not import env.memory")?,
        functions: names
            .into_iter()
            .filter(|&(_, index)| index >= imported_funcs)
            .map(|(name, index)| Ok((name, sig(func_types[index as usize])?)))
            .collect::<Result<_>>()?,
    })
}

fn conv(ty: &ValType) -> Result<Ty> {
    Ok(match ty {
        ValType::I32 => Ty::I32,
        ValType::I64 => Ty::I64,
        ValType::F32 => Ty::F32,
        ValType::F64 => Ty::F64,
        ValType::V128 => Ty::V128,
        other => bail!("unsupported value type {other:?}"),
    })
}
