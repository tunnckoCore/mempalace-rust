#[test]
fn smoke_suite_modules_compile() {
    let text = mempalace_rust::bench::backend_help();
    assert!(text.contains("recall_at_k") || text.contains("Recall@k"));
}
