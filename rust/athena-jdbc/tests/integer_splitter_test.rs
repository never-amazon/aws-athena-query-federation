use athena_jdbc::splits::{IntegerSplitter, SplitInfo, SplitRange, Splitter};
use athena_jdbc::sql::Types;

type SplitCase = (
    SplitRange<i32>,
    i32,
    Vec<SplitRange<i32>>,
    Vec<&'static str>,
);

fn range(low: i32, high: i32) -> SplitRange<i32> {
    SplitRange::new(low, high)
}

#[test]
fn split_test() {
    let cases: Vec<SplitCase> = vec![
        (
            range(1, 10),
            2,
            vec![range(1, 5), range(6, 10)],
            vec![
                "(testColumn >= 1 AND testColumn <= 5)",
                "(testColumn >= 6 AND testColumn <= 10)",
            ],
        ),
        (
            range(1, 2),
            10,
            vec![range(1, 1), range(2, 2)],
            vec![
                "(testColumn >= 1 AND testColumn <= 1)",
                "(testColumn >= 2 AND testColumn <= 2)",
            ],
        ),
        (
            range(1, 2),
            2,
            vec![range(1, 1), range(2, 2)],
            vec![
                "(testColumn >= 1 AND testColumn <= 1)",
                "(testColumn >= 2 AND testColumn <= 2)",
            ],
        ),
        (
            range(1, 10),
            1,
            vec![range(1, 10)],
            vec!["(testColumn >= 1 AND testColumn <= 10)"],
        ),
        (
            range(1, 10),
            0,
            vec![range(1, 10)],
            vec!["(testColumn >= 1 AND testColumn <= 10)"],
        ),
        (
            range(1, 10),
            -10,
            vec![range(1, 10)],
            vec!["(testColumn >= 1 AND testColumn <= 10)"],
        ),
        (
            range(1, 10),
            3,
            vec![range(1, 4), range(5, 7), range(8, 10)],
            vec![
                "(testColumn >= 1 AND testColumn <= 4)",
                "(testColumn >= 5 AND testColumn <= 7)",
                "(testColumn >= 8 AND testColumn <= 10)",
            ],
        ),
        (
            range(1, 1),
            2,
            vec![range(1, 1)],
            vec!["(testColumn >= 1 AND testColumn <= 1)"],
        ),
    ];

    for (index, (input_range, num_splits, expected_ranges, expected_clauses)) in
        cases.into_iter().enumerate()
    {
        let split_info =
            SplitInfo::new(input_range, "testColumn", Types::INTEGER, num_splits).unwrap();
        let split_ranges: Vec<SplitRange<i32>> =
            IntegerSplitter::new(split_info).unwrap().collect();
        assert_eq!(expected_ranges, split_ranges, "case {index}");

        let split_clause_info =
            SplitInfo::new(input_range, "testColumn", Types::INTEGER, num_splits).unwrap();
        let mut clause_splitter = IntegerSplitter::new(split_clause_info).unwrap();
        let mut split_clauses = Vec::new();
        while clause_splitter.has_next() {
            split_clauses.push(clause_splitter.next_range_clause());
        }
        assert_eq!(expected_clauses, split_clauses, "case {index}");
    }
}
