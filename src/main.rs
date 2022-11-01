use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{BranchType, ObjectType, Oid, Repository, Tree};

type Result<T> = std::result::Result<T, git2::Error>;

/// Blob oids of an html blob that we have already minified in the past.
#[derive(Debug)]
struct MinifiedBlobs {
    /// Oid of the minified html.
    minified: Oid,

    /// Oid of the minified and then gzipped html.
    gz: Oid,

    /// Oid of the minified and then Brotli-compressed html.
    br: Oid,

    /// Stats about the original and compressed file sizes.
    sizes: Sizes,
}

/// Sizes, in bytes, of an html document in various forms.
#[derive(Debug, Copy, Clone, Default)]
struct Sizes {
    original_len: usize,
    minified_len: usize,
    gz_len: usize,
    br_len: usize,
}

impl std::fmt::Display for Sizes {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Original: {}, Minified: {} ({:.1}%), Gzip: {} ({:.1}%), Brotli: {} ({:.1}%)",
            self.original_len,
            self.minified_len,
            100.0 * self.minified_len as f32 / self.original_len as f32,
            self.gz_len,
            100.0 * self.gz_len as f32 / self.original_len as f32,
            self.br_len,
            100.0 * self.br_len as f32 / self.original_len as f32,
        )
    }
}

impl std::ops::Add for Sizes {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            original_len: self.original_len + other.original_len,
            minified_len: self.minified_len + other.minified_len,
            gz_len: self.gz_len + other.gz_len,
            br_len: self.br_len + other.br_len,
        }
    }
}

/// A cache of minified and compressed blobs.
///
/// We use a B-tree map here instead of a hash map to ensure that we can
/// serialize in sorted order, to keep the output deterministic. The overhead
/// of the lookup is small anyway compared to compression.
struct Cache(BTreeMap<Oid, MinifiedBlobs>);

impl Cache {
    /// TSV header row for the serialization format.
    const HEADER: &'static str = "\
        blob\tblob_len\t\
        minified\tminified_len\t\
        gz\tgz_len\t\
        br\tbr_len";

    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Serialize the cache into a tab-separated values document.
    fn serialize<W: io::Write>(&self, mut out: W) -> std::io::Result<()> {
        writeln!(out, "{}", Self::HEADER)?;
        for (k, v) in self.0.iter() {
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                k.to_string(),
                v.sizes.original_len,
                v.minified.to_string(),
                v.sizes.minified_len,
                v.gz.to_string(),
                v.sizes.gz_len,
                v.br.to_string(),
                v.sizes.br_len,
            )?;
        }
        Ok(())
    }

    /// Read the cache from a tab-separated values document.
    fn deserialize<R: io::BufRead>(input: R) -> std::io::Result<Self> {
        use std::str::FromStr;

        let mut result = BTreeMap::new();
        let mut lines = input.lines();

        // Skip but verify the header row, it is just there for clarity.
        match lines.next() {
            None => panic!("Failed to load cache, expected header row."),
            Some(row) => assert_eq!(row?, Self::HEADER, "Invalid header row."),
        }

        for line_opt in lines {
            let line = line_opt?;

            let as_oid = |part: Option<&str>| {
                Oid::from_str(part.expect("Invalid format, expected oid.")).expect("Invalid oid.")
            };
            let as_usize = |part: Option<&str>| {
                usize::from_str(part.expect("Invalid format, expected len.")).expect("Invalid len.")
            };

            let mut parts = line.split('\t');

            let key = as_oid(parts.next());
            let original_len = as_usize(parts.next());
            let minified = as_oid(parts.next());
            let minified_len = as_usize(parts.next());
            let gz = as_oid(parts.next());
            let gz_len = as_usize(parts.next());
            let br = as_oid(parts.next());
            let br_len = as_usize(parts.next());

            result.insert(
                key,
                MinifiedBlobs {
                    minified,
                    gz,
                    br,
                    sizes: Sizes {
                        original_len,
                        minified_len,
                        gz_len,
                        br_len,
                    },
                },
            );
        }

        Ok(Cache(result))
    }

    /// Save the cache to the given tsv file.
    pub fn save(&self, fname: &str) -> io::Result<()> {
        let f = fs::File::create(fname)?;
        let writer = io::BufWriter::new(f);
        self.serialize(writer)
    }

    /// Load a cache from the given tsv file.
    pub fn load(fname: &str) -> io::Result<Self> {
        let f = fs::File::open(fname)?;
        let reader = io::BufReader::new(f);
        Self::deserialize(reader)
    }
}

/// Gzip-compress the input using Zopfli at high compression (slow to run).
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

/// Brotli-compress the input at maximum compression level.
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

/// Minify html and embedded CSS. Preserves a license comment.
fn minify_html(input: &[u8]) -> Vec<u8> {
    use std::str;

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

    let minified_bytes = minify_html::minify(input, &cfg);

    let minified_str = str::from_utf8(&minified_bytes[..])
        .expect("File should be valid UTF-8.");

    // Put back the copyright notices that minification would strip.
    minified_str.replace(
        "<html><head>",
        "<html><!--\n\
        Kilsbergen MkDocs theme copyright 2022 Ruud van Asseldonk,\n\
        licensed Apache 2.0, https://github.com/ruuda/kilsbergen.\n\
        Inter font family copyright Rasmus Andersson,\n\
        licensed SIL OFL 1.1, https://rsms.me/inter/.\n--><head>"
    ).into_bytes()
}

/// Minimize and compress a blob that contains html.
fn minimize_blob(repo: &Repository, id: Oid) -> Result<MinifiedBlobs> {
    let blob = repo.find_blob(id)?;


    let mut stdout = std::io::stdout().lock();
    let mut print_status = |status| {
        use std::io::Write;
        write!(stdout, "\r{:?}: {}", id, status).unwrap();
        stdout.flush().unwrap();
    };

    print_status("minify");
    let minified_bytes = minify_html(blob.content());
    print_status("zopfli");
    let gz_bytes = compress_zopfli(&minified_bytes[..]);
    print_status("brotli");
    let br_bytes = compress_brotli(&minified_bytes[..]);
    print_status("complete\n");

    // Store the minified version in a blob.
    let result = MinifiedBlobs {
        minified: repo.blob(&minified_bytes[..])?,
        gz: repo.blob(&gz_bytes[..])?,
        br: repo.blob(&br_bytes[..])?,
        sizes: Sizes {
            original_len: blob.size(),
            minified_len: minified_bytes.len(),
            gz_len: gz_bytes.len(),
            br_len: br_bytes.len(),
        },
    };

    Ok(result)
}

/// Like [`minimize_blob`], but return blobs from the cache if possible.
///
/// Also fills the cache for blobs that we minimized/compressed for the first
/// time.
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

/// Given a Git tree, make a copy where all html files are compressed.
///
/// This minifies .html files, and adds a Gzip and Brotli compressed version as
/// well. Non-interesting files are dropped from the tree.
fn minimize_tree(
    cache: &mut Cache,
    sizes: &mut Sizes,
    repo: &Repository,
    tree: &Tree,
    depth: u32,
) -> Result<Option<Oid>> {
    let base_tree = None;
    let mut builder = repo.treebuilder(base_tree)?;

    let filemode_directory = 0o040000;
    let filemode_regular = 0o0100644;

    for entry in tree.iter() {
        let name = entry.name().expect("Invalid name in tree entry.");

        match entry.kind() {
            Some(ObjectType::Tree) => {
                // Skip the theme, MkDocs includes this because I put the theme
                // in a subdirectory of the docs, but it really shouldn't be
                // there.
                if name == "theme" && depth == 0 {
                    continue;
                }

                let subtree = repo.find_tree(entry.id())?;
                if let Some(sub_oid) = minimize_tree(cache, sizes, repo, &subtree, depth + 1)? {
                    builder.insert(name, sub_oid, filemode_directory)?;
                }
            }
            Some(ObjectType::Blob) => {
                if name.ends_with(".html") {
                    let blobs = minimize_blob_cached(cache, repo, entry.id())?;
                    builder.insert(name, blobs.minified, filemode_regular)?;
                    builder.insert(format!("{name}.gz"), blobs.gz, filemode_regular)?;
                    builder.insert(format!("{name}.br"), blobs.br, filemode_regular)?;
                    *sizes = *sizes + blobs.sizes;
                }
                if name.ends_with(".png") || name.ends_with(".jpg") {
                    builder.insert(name, entry.id(), filemode_regular)?;
                }
            }
            ot => panic!("Unexpected object type in tree: {:?}", ot),
        }
    }

    if builder.is_empty() {
        Ok(None)
    } else {
        let tree_oid = builder.write()?;
        Ok(Some(tree_oid))
    }
}

fn minimize(cache: &mut Cache, repo: &Repository) -> Result<Oid> {
    let pages_branch = repo.find_branch("gh-pages", BranchType::Local)?;
    println!("Branch gh-pages -> {:?}", pages_branch.get().target().unwrap());
    let tree = pages_branch.get().peel_to_tree()?;

    let initial_depth = 0;
    let mut sizes = Sizes::default();
    let tree_min = minimize_tree(cache, &mut sizes, repo, &tree, initial_depth)?.expect("Must have a root tree.");
    println!("Minimized tree  -> {:?}", tree_min);
    println!("{}", sizes);

    Ok(tree_min)
}

/// Check out the given tree at the given path.
///
/// This is a destructive function that clears whatever is currently at that
/// path.
fn checkout_into<P: AsRef<Path>>(repo: &Repository, root: Oid, target_dir: P) -> Result<()> {
    let mut checkout_builder = CheckoutBuilder::new();
    checkout_builder
        .target_dir(target_dir.as_ref())
        .update_index(false)
        .remove_ignored(true)
        .remove_untracked(true)
        .force();
    let root_obj = repo.find_object(root, Some(ObjectType::Tree))?;
    repo.checkout_tree(&root_obj, Some(&mut checkout_builder))
}

fn main() -> Result<()> {
    let mut args = std::env::args();
    // Skip the program name.
    args.next();

    let repo_path = args.next().expect("Expected repository path.");
    let repo = Repository::open(repo_path)?;

    let target_path = args.next().expect("Expected target path.");

    let mut cache = match Cache::load("cache.tsv") {
        Ok(cache) => cache,
        Err(_) => {
            println!("Starting with empty cache, cache failed to load.");
            Cache::new()
        }
    };

    let root_tree = minimize(&mut cache, &repo)?;

    cache.save("cache.tsv.new").expect("Failed to save cache.");
    std::fs::rename("cache.tsv.new", "cache.tsv").expect("Failed to move cache.");

    // TODO: Create a ref to avoid the root getting GC'd.

    checkout_into(&repo, root_tree, &target_path)?;
    println!("Checked out tree {:?} at {}.", root_tree, target_path);

    Ok(())
}
