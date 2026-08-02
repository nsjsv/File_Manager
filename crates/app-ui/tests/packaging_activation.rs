use std::fs;
use std::path::Path;

fn repository_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("app-ui crate is nested under crates")
}

#[test]
fn desktop_and_brand_activation_files_expose_local_file_manager_contract() {
    let root = repository_root();
    let desktop_entry = fs::read_to_string(root.join("packaging/linux/file-manager.desktop"))
        .expect("read desktop entry");
    assert!(desktop_entry
        .lines()
        .any(|line| line == "Exec=file-manager %F"));
    assert!(desktop_entry
        .lines()
        .any(|line| line == "MimeType=inode/directory;"));

    let service_name = "io.github.nsjsv.FileManager.service";
    let activation_service = fs::read_to_string(root.join("packaging/linux").join(service_name))
        .expect("read activation service");
    assert!(activation_service
        .lines()
        .any(|line| line == "Name=io.github.nsjsv.FileManager"));
    assert!(activation_service
        .lines()
        .any(|line| line == "Exec=/usr/bin/file-manager --activation-service"));

    let release_workflow = fs::read_to_string(root.join(".github/workflows/release.yml"))
        .expect("read release workflow");
    assert!(release_workflow.contains("usr/share/dbus-1/services/${ACTIVATION_SERVICE_FILE}"));
}

#[test]
fn packaged_dbus_services_do_not_claim_the_standard_file_manager_name() {
    let service_directory = repository_root().join("packaging/linux");
    for entry in fs::read_dir(service_directory).expect("read Linux packaging directory") {
        let path = entry.expect("packaging entry").path();
        if path
            .extension()
            .is_some_and(|extension| extension == "service")
        {
            let service = fs::read_to_string(&path).expect("read service file");
            assert!(
                !service
                    .lines()
                    .any(|line| line == "Name=org.freedesktop.FileManager1"),
                "{} must not claim the standard FileManager1 name",
                path.display()
            );
        }
    }
}
