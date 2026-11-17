use os::kg::{self, Ontology, Paths};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const GUIDES: [&str; 6] = [
    "rust-type-driven-design",
    "rust-resource-and-concurrency-patterns",
    "typescript-domain-patterns",
    "typescript-async-and-resilience-patterns",
    "verification-patterns",
    "architecture-fitness-and-quality-gates",
];

#[test]
fn engineering_guides_are_discoverable_valid_and_connected() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let template = root.join("template");
    let concepts = template.join("concepts");
    let library = fs::read_to_string(concepts.join("library.md")).unwrap();
    let skill = fs::read_to_string(root.join("skills/quality-gate/SKILL.md")).unwrap();
    let ontology = Ontology::load(&template.join("graph/ontology.yaml")).unwrap();
    let paths = Paths { root: template.clone() };
    let known: HashMap<String, String> = kg::wiki_pages(&template)
        .into_iter()
        .map(|(id, kind, _)| (id, kind))
        .collect();
    let links = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();

    for slug in GUIDES {
        assert!(library.contains(&format!("[[{slug}]]")), "library does not route to {slug}");
        assert!(skill.contains(slug), "quality-gate does not select {slug}");

        let relative = format!("concepts/{slug}.md");
        let page = kg::frontmatter(&concepts.join(format!("{slug}.md")));
        for field in ["title", "summary", "updated"] {
            assert!(!page.text(field).is_empty(), "{relative}: missing {field}");
        }
        assert_eq!(page.text("type"), "concept", "{relative}: wrong node type");
        assert!(os::clock::parse_date(&page.text("updated")).is_some(), "{relative}: invalid updated date");
        let normalized_body = page.body.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(normalized_body.contains("No immutable source"), "{relative}: provenance status is unstated");

        for edge in page.list("edges") {
            let (relation, target) = edge.split_once(' ').unwrap();
            let target = kg::slug(target);
            let rule = ontology.relations.get(relation).unwrap();
            assert!(rule.accepts_domain("concept"), "{relative}: invalid edge domain");
            let target_kind = known.get(&target).unwrap_or_else(|| panic!("{relative}: unknown edge target {target}"));
            assert!(rule.accepts_range(target_kind), "{relative}: invalid edge range");
        }

        for capture in links.captures_iter(&page.body) {
            let target = kg::slug(capture.get(1).unwrap().as_str());
            assert!(known.contains_key(&target), "{relative}: dangling wiki link {target}");
        }
    }

    let pull = os::sare::extract::pages(&paths, &ontology);
    let guide_problems: Vec<_> = pull
        .problems
        .iter()
        .filter(|problem| GUIDES.iter().any(|slug| problem.contains(&format!("concepts/{slug}.md"))))
        .collect();
    assert!(guide_problems.is_empty(), "guide extraction failed: {guide_problems:?}");
}
