/*
 * Copyright (C) 2019 Intel Corporation. All rights reserved.
 * SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
 */

//! .wasm compiled, in-memory representation
//! get one via `Module::from_file()` or `Module::from_buf()`

use crate::{
    helper::error_buf_to_string, helper::DEFAULT_ERROR_BUF_SIZE, runtime::Runtime,
    wasi_context::WasiCtx, RuntimeError,
};
use std::{fs::File, io::Read, path::Path, ptr, string::String, vec::Vec};
use wamr_sys::{
    wasm_module_t, wasm_runtime_load, wasm_runtime_set_wasi_addr_pool, wasm_runtime_set_wasi_args,
    wasm_runtime_set_wasi_ns_lookup_pool, wasm_runtime_unload,
};

#[allow(dead_code)]
#[derive(Debug)]
pub struct Module {
    module: wasm_module_t,
    // to keep the module content in memory
    content: Vec<u8>,
    wasi_ctx: WasiCtx,
}

impl Module {
    /// compile a module with the given wasm file path
    ///
    /// # Error
    ///
    /// If the file does not exist or the file cannot be read, an `RuntimeError::WasmFileFSError` will be returned.
    /// If the wasm file is not a valid wasm file, an `RuntimeError::CompilationError` will be returned.
    pub fn from_file(runtime: &Runtime, wasm_file: &Path) -> Result<Self, RuntimeError> {
        let mut wasm_file = File::open(wasm_file)?;

        let mut binary: Vec<u8> = Vec::new();
        wasm_file.read_to_end(&mut binary)?;

        Self::from_buf(runtime, &binary)
    }

    /// compile a module int the given buffer
    ///
    /// # Error
    ///
    /// If the file does not exist or the file cannot be read, an `RuntimeError::WasmFileFSError` will be returned.
    /// If the wasm file is not a valid wasm file, an `RuntimeError::CompilationError` will be returned.
    pub fn from_buf(_runtime: &Runtime, buf: &[u8]) -> Result<Self, RuntimeError> {
        let mut content = buf.to_vec();
        let mut error_buf = [0i8; DEFAULT_ERROR_BUF_SIZE];
        let module = unsafe {
            wasm_runtime_load(
                content.as_mut_ptr(),
                content.len() as u32,
                error_buf.as_mut_ptr(),
                error_buf.len() as u32,
            )
        };

        if module.is_null() {
            match error_buf.len() {
                0 => {
                    return Err(RuntimeError::CompilationError(String::from(
                        "load module failed",
                    )))
                }
                _ => {
                    return Err(RuntimeError::CompilationError(error_buf_to_string(
                        &error_buf,
                    )))
                }
            }
        }

        Ok(Module {
            module,
            content,
            wasi_ctx: WasiCtx::default(),
        })
    }

    /// set Wasi context for a module
    ///
    /// This function should be called before `Instance::new`
    pub fn set_wasi_context(&mut self, wasi_ctx: WasiCtx) {
        self.wasi_ctx = wasi_ctx;

        let real_paths = if self.wasi_ctx.get_preopen_real_paths().is_empty() {
            ptr::null_mut()
        } else {
            self.wasi_ctx.get_preopen_real_paths().as_ptr() as *mut *const i8
        };

        let mapped_paths = if self.wasi_ctx.get_preopen_mapped_paths().is_empty() {
            ptr::null_mut()
        } else {
            self.wasi_ctx.get_preopen_mapped_paths().as_ptr() as *mut *const i8
        };

        let env = if self.wasi_ctx.get_env_vars().is_empty() {
            ptr::null_mut()
        } else {
            self.wasi_ctx.get_env_vars().as_ptr() as *mut *const i8
        };

        let args = if self.wasi_ctx.get_arguments().is_empty() {
            ptr::null_mut()
        } else {
            self.wasi_ctx.get_arguments().as_ptr() as *mut *mut i8
        };

        unsafe {
            wasm_runtime_set_wasi_args(
                self.get_inner_module(),
                real_paths,
                self.wasi_ctx.get_preopen_real_paths().len() as u32,
                mapped_paths,
                self.wasi_ctx.get_preopen_mapped_paths().len() as u32,
                env,
                self.wasi_ctx.get_env_vars().len() as u32,
                args,
                self.wasi_ctx.get_arguments().len() as i32,
            );

            let ns_lookup_pool = if self.wasi_ctx.get_allowed_dns().is_empty() {
                ptr::null_mut()
            } else {
                self.wasi_ctx.get_allowed_dns().as_ptr() as *mut *const i8
            };

            wasm_runtime_set_wasi_ns_lookup_pool(
                self.get_inner_module(),
                ns_lookup_pool,
                self.wasi_ctx.get_allowed_dns().len() as u32,
            );

            let addr_pool = if self.wasi_ctx.get_allowed_address().is_empty() {
                ptr::null_mut()
            } else {
                self.wasi_ctx.get_allowed_address().as_ptr() as *mut *const i8
            };
            wasm_runtime_set_wasi_addr_pool(
                self.get_inner_module(),
                addr_pool,
                self.wasi_ctx.get_allowed_address().len() as u32,
            );
        }
    }

    pub fn get_inner_module(&self) -> wasm_module_t {
        self.module
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            wasm_runtime_unload(self.module);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{runtime::Runtime, wasi_context::WasiCtxBuilder};
    use std::path::PathBuf;

    #[test]
    fn test_module_not_exist() {
        let runtime = Runtime::new();
        assert!(runtime.is_ok());

        let runtime = runtime.unwrap();

        let module = Module::from_file(&runtime, Path::new("not_exist"));
        assert!(module.is_err());
    }

    #[test]
    fn test_module_from_buf() {
        let runtime = Runtime::new().unwrap();

        // (module
        //   (func (export "add") (param i32 i32) (result i32)
        //     (local.get 0)
        //     (local.get 1)
        //     (i32.add)
        //   )
        // )
        let binary = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f,
            0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64,
            0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
        ];
        let binary = binary.into_iter().map(|c| c as u8).collect::<Vec<u8>>();

        let module = Module::from_buf(&runtime, &binary);
        assert!(module.is_ok());
    }

    #[test]
    fn test_module_from_file() {
        let runtime = Runtime::new().unwrap();

        let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        d.push("resources/test");
        d.push("gcd_wasm32_wasi.wasm");
        let module = Module::from_file(&runtime, d.as_path());
        assert!(module.is_ok());
    }

    #[test]
    fn test_module_with_wasi_args() {
        let runtime = Runtime::new().unwrap();

        // (module
        //   (func (export "add") (param i32 i32) (result i32)
        //     (local.get 0)
        //     (local.get 1)
        //     (i32.add)
        //   )
        // )
        let binary = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f,
            0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64,
            0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
        ];
        let binary = binary.into_iter().map(|c| c as u8).collect::<Vec<u8>>();

        let module = Module::from_buf(&runtime, &binary);
        assert!(module.is_ok());
        let mut module = module.unwrap();

        let wasi_ctx = WasiCtxBuilder::new()
            .set_pre_open_path(vec!["."], vec![])
            .set_env_vars(vec![])
            .set_allowed_address(vec![])
            .set_allowed_dns(vec![])
            .build();

        module.set_wasi_context(wasi_ctx);
    }
}
