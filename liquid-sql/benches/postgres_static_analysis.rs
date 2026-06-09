use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use liquid_sql::{PgSqlAnalysisRequest, analyze_postgres_sql};

fn bench_static_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("postgres_static_analysis");

    let cases = [
        ("safe_select", "select id, email from users where id = 42"),
        ("delete_without_where", "delete from users"),
        ("parse_error", "select from where"),
        (
            "nested_query_risks",
            "with exported as (select * from users) \
             select u.id from (select * from users) u cross join orders",
        ),
        (
            "mixed_risk_script",
            "select * from users; \
             update users set role = 'admin'; \
             drop table old_events cascade; \
             grant select on table users to analyst; \
             lock table users in access exclusive mode",
        ),
    ];

    for (name, sql) in cases {
        group.bench_function(name, |b| {
            b.iter_batched(
                || PgSqlAnalysisRequest::new(sql),
                |request| black_box(analyze_postgres_sql(black_box(request))),
                BatchSize::SmallInput,
            );
        });
    }

    for row_count in [10_usize, 100, 1_000] {
        let sql = insert_values_sql(row_count);
        group.bench_with_input(
            BenchmarkId::new("insert_values_rows", row_count),
            &sql,
            |b, sql| {
                b.iter_batched(
                    || PgSqlAnalysisRequest::new(sql.clone()),
                    |request| black_box(analyze_postgres_sql(black_box(request))),
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn insert_values_sql(row_count: usize) -> String {
    let values = (0..row_count)
        .map(|index| format!("({index}, 'user{index}@example.com')"))
        .collect::<Vec<_>>()
        .join(", ");

    format!("insert into users(id, email) values {values}")
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_static_analysis
}
criterion_main!(benches);
