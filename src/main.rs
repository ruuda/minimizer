use std::collections::BTreeMap;
use std::fs;
use std::io;

use git2::{BranchType, ObjectType, Oid, Repository, Tree};

type Result<T> = std::result::Result<T, git2::Error>;

/// Blob oids of an html blob that we have already minified in the past.
#[derive(Debug)]
struct MinifiedBlobs {
    minified: Oid,
    gz: Oid,
    br: Oid,
}

/// A cache of minified and comprssed blobs.
///
/// We use a B-tree map here instead of a hash map to ensure that we can
/// serialize in sorted order, to keep the output deterministic. The overhead
/// of the lookup is small anyway compared to compression.
struct Cache(BTreeMap<Oid, MinifiedBlobs>);

impl Cache {
    const HEADER: &'static str = "blob\tminified\tgz\tbr";

    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    fn serialize<W: io::Write>(&self, mut out: W) -> std::io::Result<()> {
        writeln!(out, "{}", Self::HEADER)?;
        for (k, v) in self.0.iter() {
            writeln!(
                out,
                "{}\t{}\t{}\t{}",
                k.to_string(),
                v.minified.to_string(),
                v.gz.to_string(),
                v.br.to_string(),
            )?;
        }
        Ok(())
    }

    fn deserialize<R: io::BufRead>(input: R) -> std::io::Result<Self> {
        let mut result = BTreeMap::new();
        let mut lines = input.lines();

        match lines.next() {
            None => panic!("Failed to load cache, expected header row."),
            Some(row) => assert_eq!(row?, Self::HEADER, "Invalid header row."),
        }

        // Skip the header row, it is just for clarity.
        for line_opt in lines {
            let line = line_opt?;
            let mut parts = line.split('\t');
            let mut next_oid = || {
                Oid::from_str(parts.next().expect("Invalid format, expected oid."))
                    .expect("Invalid oid.")
            };
            result.insert(
                next_oid(),
                MinifiedBlobs {
                    minified: next_oid(),
                    gz: next_oid(),
                    br: next_oid(),
                },
            );
        }

        Ok(Cache(result))
    }

    pub fn save(&self, fname: &str) -> io::Result<()> {
        let f = fs::File::create(fname)?;
        let writer = io::BufWriter::new(f);
        self.serialize(writer)
    }

    pub fn load(fname: &str) -> io::Result<Self> {
        let f = fs::File::open(fname)?;
        let reader = io::BufReader::new(f);
        Self::deserialize(reader)
    }
}

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
    use io::Write;
    let level = 11;
    let mut encoder = brotli2::write::BrotliEncoder::new(Vec::new(), level);
    encoder
        .write_all(input)
        .expect("No IO happens here, should not fail.");
    encoder
        .finish()
        .expect("No IO happens here, should not fail.")
}

fn minimize_blob(repo: &Repository, id: Oid) -> Result<MinifiedBlobs> {
    let blob = repo.find_blob(id)?;

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
    let gz_bytes = compress_zopfli(&minified_bytes[..]);
    let br_bytes = compress_brotli(&minified_bytes[..]);

    println!(
        "  -> shrunk {} to {} ({:.1}%), gzipped to {} ({:.1}%), brotlid to {} ({:.1}%)",
        blob.size(),
        minified_bytes.len(),
        100.0 * minified_bytes.len() as f32 / blob.size() as f32,
        gz_bytes.len(),
        100.0 * gz_bytes.len() as f32 / blob.size() as f32,
        br_bytes.len(),
        100.0 * br_bytes.len() as f32 / blob.size() as f32,
    );

    // Store the minified version in a blob.
    let result = MinifiedBlobs {
        minified: repo.blob(&minified_bytes[..])?,
        gz: repo.blob(&gz_bytes[..])?,
        br: repo.blob(&br_bytes[..])?,
    };

    Ok(result)
}

fn minimize_blob_cached<'a>(
    cache: &'a mut Cache,
    repo: &Repository,
    id: Oid,
) -> Result<&'a MinifiedBlobs> {
    use std::collections::btree_map::Entry;

    let blobs = match cache.0.entry(id) {
        Entry::Occupied(o) => o.into_mut(),
        Entry::Vacant(v) => v.insert(minimize_blob(repo, id)?),
    };

    Ok(blobs)
}

fn minimize_tree(cache: &mut Cache, repo: &Repository, tree: &Tree) -> Result<()> {
    for entry in tree.iter() {
        println!(
            "{:?} {}",
            entry.id(),
            entry.name().expect("Invalid filename")
        );

        match entry.kind() {
            Some(ObjectType::Tree) => {
                let subtree = repo.find_tree(entry.id())?;
                minimize_tree(cache, repo, &subtree)?;
            }
            Some(ObjectType::Blob) => {
                let name = entry.name().expect("Huh, unnamed tree entry?");
                if name.ends_with(".html") {
                    let blobs = minimize_blob_cached(cache, repo, entry.id())?;
                    println!(" -> {:?}", blobs);
                }
            }
            ot => panic!("Unexpected object type in tree: {:?}", ot),
        }
    }

    Ok(())
}

fn minimize(cache: &mut Cache, repo: &Repository) -> Result<()> {
    let pages_branch = repo.find_branch("gh-pages", BranchType::Local)?;
    println!("Branch: {:?}", pages_branch.get().target());
    let tree = pages_branch.get().peel_to_tree()?;

    minimize_tree(cache, repo, &tree)?;

    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args();
    // Skip the program name.
    args.next();

    let repo_path = args.next().expect("Expected repository path.");
    let repo = Repository::open(repo_path)?;

    let mut cache = match Cache::load("cache.tsv") {
        Ok(cache) => cache,
        Err(err) => {
            println!("Starting with empty cache, cache failed to load: {:?}", err);
            Cache::new()
        }
    };

    minimize(&mut cache, &repo)?;

    cache.save("cache.tsv.new").expect("Failed to save cache.");
    std::fs::rename("cache.tsv.new", "cache.tsv").expect("Failed to move cache.");

    Ok(())
}
