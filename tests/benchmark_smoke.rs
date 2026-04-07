#[test]
fn benchmark_runner_smoke() {
    let dir = tempfile::tempdir().expect("tempdir");
    let palace = dir.path().join("palace");
    let mut storage = mempalace_rust::storage::Storage::open(&palace).expect("open storage");
    let dataset = mempalace_rust::bench::BenchmarkDataset {
        documents: vec![
            mempalace_rust::bench::BenchmarkDoc {
                id: "doc1".to_string(),
                wing: "alpha".to_string(),
                room: "architecture".to_string(),
                content: "We chose GraphQL for typed queries".to_string(),
                source_file: "bench/doc1.md".to_string(),
            },
            mempalace_rust::bench::BenchmarkDoc {
                id: "doc2".to_string(),
                wing: "alpha".to_string(),
                room: "general".to_string(),
                content: "We discussed generic API ideas".to_string(),
                source_file: "bench/doc2.md".to_string(),
            },
        ],
        queries: vec![mempalace_rust::bench::BenchmarkCase {
            id: "q1".to_string(),
            query: "typed GraphQL queries".to_string(),
            relevant_ids: vec!["doc1".to_string()],
        }],
    };
    let result = mempalace_rust::bench::run_benchmark(
        &mut storage,
        &dataset,
        mempalace_rust::bench::BenchmarkBackend::Hybrid,
        5,
    )
    .expect("run benchmark");
    assert_eq!(result.queries, 1);
    assert!(result.recall_at_k >= 0.0);

    let mut storage2 =
        mempalace_rust::storage::Storage::open(&dir.path().join("palace2")).expect("open storage2");
    let changed = mempalace_rust::bench::run_benchmark(
        &mut storage2,
        &mempalace_rust::bench::BenchmarkDataset {
            documents: vec![mempalace_rust::bench::BenchmarkDoc {
                id: "doc1".to_string(),
                wing: "alpha".to_string(),
                room: "architecture".to_string(),
                content: "We chose REST instead".to_string(),
                source_file: "bench/doc1.md".to_string(),
            }],
            queries: vec![mempalace_rust::bench::BenchmarkCase {
                id: "q1".to_string(),
                query: "REST instead".to_string(),
                relevant_ids: vec!["doc1".to_string()],
            }],
        },
        mempalace_rust::bench::BenchmarkBackend::Hybrid,
        5,
    )
    .expect("run changed benchmark");
    assert!(changed.recall_at_k >= 0.0);
}
