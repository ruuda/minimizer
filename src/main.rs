use git2::{BranchType, ObjectType, Oid, Repository, Tree};

type Result<T> = std::result::Result<T, git2::Error>;

fn compress_zopfli(input: &[u8]) -> Vec<u8> {
    let opts = zopfli::Options {
        // Be slow but compress well, only really feasible for small files, but
        // my html files are small, so that's fine.
        iteration_count: std::num::NonZeroU8::new(20).unwrap(),
        // Not sure what this does, use the default value.
        maximum_block_splits: 15,
    };
    let mut output = Vec::new();
    let input = std::io::Cursor::new(input);
    zopfli::compress(&opts, &zopfli::Format::Gzip, input, &mut output)
        .expect("Zopfli compression should not fail, we don't do IO here.");

    output
}

fn compress_brotli(input: &[u8]) -> Vec<u8> {
    todo!()
}

fn minimize_blob(repo: &Repository, _name: &str, id: Oid) -> Result<Oid> {
    let blob = repo.find_blob(id)?;

    // TODO: Cache this.

    let cfg = minify_html::Cfg {
        do_not_minify_doctype: true,
        ensure_spec_compliant_unquoted_attribute_values: true,
        keep_closing_tags: true,
        keep_html_and_head_opening_tags: true,
        keep_spaces_between_attributes: true,
        keep_comments: false,
        minify_css: true,
        minify_js: false,
        remove_bangs: false,
        remove_processing_instructions: true,
    };
    let minified_bytes = minify_html::minify(blob.content(), &cfg);
    let gzip_bytes = compress_zopfli(&minified_bytes[..]);

    // Store the minified version in a blob.
    let blob_raw = repo.blob(&minified_bytes[..])?;

    println!(
        "  -> shrunk {} to {} ({:.1}%), gzipped to {} ({:.1}%)",
        blob.size(),
        minified_bytes.len(),
        100.0 * minified_bytes.len() as f32 / blob.size() as f32,
        gzip_bytes.len(),
        100.0 * gzip_bytes.len() as f32 / blob.size() as f32,
    );

    // TODO: Actually, instead of returning the oid, we should probably append
    // an entry to some in-progress tree.
    Ok(blob_raw)
}

fn minimize_tree(repo: &Repository, tree: &Tree) -> Result<()> {
    for entry in tree.iter() {
        println!(
            "{:?} {}",
            entry.id(),
            entry.name().expect("Invalid filename")
        );

        match entry.kind() {
            Some(ObjectType::Tree) => {
                let subtree = repo.find_tree(entry.id())?;
                minimize_tree(repo, &subtree)?;
            }
            Some(ObjectType::Blob) => {
                let name = entry.name().expect("Huh, unnamed tree entry?");
                if name.ends_with(".html") {
                    let new_oid = minimize_blob(repo, name, entry.id())?;
                    println!(" -> {:?}", new_oid);
                }
            }
            ot => panic!("Unexpected object type in tree: {:?}", ot),
        }
    }

    Ok(())
}

fn minimize(repo: &mut Repository) -> Result<()> {
    let pages_branch = repo.find_branch("gh-pages", BranchType::Local)?;
    println!("Branch: {:?}", pages_branch.get().target());
    let tree = pages_branch.get().peel_to_tree()?;

    minimize_tree(&repo, &tree)?;

    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args();
    // Skip the program name.
    args.next();

    let repo_path = args.next().expect("Expected repository path.");

    let mut repo = Repository::open(repo_path)?;
    minimize(&mut repo)?;

    Ok(())
}
