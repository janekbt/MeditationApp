fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(std::collections::HashMap::from([(
            "material".to_string(),
            std::path::Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
                .join("material-1.0/material.slint"),
        )]))
        // i18n (P8): bundle lang/<lang>/LC_MESSAGES/meditate-android.po
        // into the binary; runtime picks via select_bundled_translation
        // from the system locale. Context disabled so the po files use
        // plain msgids (shared/harvested from the GTK shell's po dir).
        .with_bundled_translations("lang")
        .with_default_translation_context(
            slint_build::DefaultTranslationContext::None,
        );
    slint_build::compile_with_config("ui/main.slint", config).unwrap();
}
