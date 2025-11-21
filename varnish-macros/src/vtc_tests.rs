use proc_macro2 as pm2;
use quote::quote;
use syn::{parse::{Parse, ParseStream}, LitStr, LitBool};

/// Sanitize a filename to create a valid Rust identifier
fn sanitize_identifier(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Input for the `run_vtc_tests` macro
pub struct VtcTestsInput {
    pub glob_pattern: LitStr,
    pub debug: Option<LitBool>,
}

impl Parse for VtcTestsInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let glob_pattern: LitStr = input.parse()?;
        let debug = if input.parse::<syn::Token![,]>().is_ok() {
            Some(input.parse()?)
        } else {
            None
        };
        Ok(VtcTestsInput { glob_pattern, debug })
    }
}

pub fn generate_vtc_tests(input: pm2::TokenStream) -> pm2::TokenStream {
    let VtcTestsInput { glob_pattern, debug } = match syn::parse2(input) {
        Ok(v) => v,
        Err(e) => return e.into_compile_error(),
    };

    let glob_str = glob_pattern.value();
    let debug_val = debug.as_ref().map(|b| b.value()).unwrap_or(false);

    // Get the manifest directory of the crate calling this macro
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    let full_pattern = std::path::Path::new(&manifest_dir).join(&glob_str);
    let full_pattern_str = full_pattern.to_string_lossy();

    // Expand glob pattern at compile time
    let test_files = match glob::glob(&full_pattern_str) {
        Ok(paths) => paths.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(e) => {
            let err_msg = format!("Failed to parse glob pattern '{}': {}", full_pattern_str, e);
            return quote! {
                compile_error!(#err_msg);
            };
        }
    };

    if test_files.is_empty() {
        let err_msg = format!("No VTC test files found matching pattern: {}", full_pattern_str);
        return quote! {
            compile_error!(#err_msg);
        };
    }

    // Generate a test function for each VTC file
    let test_functions = test_files.iter().map(|path| {
        let path_str = path.to_string_lossy();
        let file_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        // Create a valid Rust identifier from the filename
        let sanitized_name = sanitize_identifier(file_name);
        let test_name = syn::Ident::new(
            &format!("vtc_{}", sanitized_name),
            pm2::Span::call_site(),
        );

        quote! {
            #[test]
            fn #test_name() {
                let vmod_lib_name = format!(
                    "{}{}{}",
                    ::std::env::consts::DLL_PREFIX,
                    env!("CARGO_PKG_NAME"),
                    ::std::env::consts::DLL_SUFFIX
                );
                let vmod_path = ::varnish::varnishtest::find_vmod_lib(
                    &vmod_lib_name,
                    env!("LD_LIBRARY_PATH"),
                )
                .expect("Failed to find VMOD library");

                ::varnish::varnishtest::run_varnish_test(
                    &vmod_path,
                    &::std::path::PathBuf::from(#path_str),
                    option_env!("VARNISHTEST_DURATION").unwrap_or("5s"),
                    #debug_val,
                )
                .unwrap();
            }
        }
    });

    quote! {
        #(#test_functions)*
    }
}
