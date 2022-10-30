# Minimizer

Minimizer is a utility that minimizes and compresses html of an [MkDocs][mkdocs]
site. It is made to work with the [Kilsbergen][kilsbergen] theme. I use
Minimizer to deploy my documentation sites, the tool is intentionally not
generic.

[mkdocs]:     https://www.mkdocs.org/
[kilsbergen]: https://github.com/ruuda/kilsbergen

## Operation

Minimizer acts on Git repositories, it creates a minimized tree from a given
tree that the `gh-pages` branch points to, and then checks it out in a
particular location. By leveraging Git, we get:

 * A content-addressable store for minified pages, so we can cache them.
 * The ability to quickly switch between different versions of the site.
 * A convenient way to make a given directory match the site with minimal
   file system operations.

## Usage

    cargo build --release
    target/release/minimizer <input-repo> <output-directory>

