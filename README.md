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
 * A convenient way to make a given directory match the site.

The minimizer generates:

 * A minified version of every .html file. It also minifies inline css.
 * A gzipped version of the minified html, generated with the [Zopfli][zopfli]
   compressor, which is slow but achieves better density than zlib.
 * A [Brotli][brotli]-compressed version of the minified html.

The compressed versions can then be used by the [`gzip_static`][gzstatic] and
`brotli_static` modules in Nginx.

[zopfli]:   https://github.com/google/zopfli
[brotli]:   https://github.com/google/brotli
[gzstatic]: https://nginx.org/en/docs/http/ngx_http_gzip_static_module.html

## Usage

The following will minimize the `gh-pages` branch of `<input-repo>` and check
out the result into `<output-directory>`. The output directory does not need to
be a Git repository.

    cargo build --release
    target/release/minimizer <input-repo> <output-directory>

A call to `minimizer` is useful to set up in a [post-receive hook][hook],
especially when combined with `mkdocs gh-deploy`. I personally use this like so:

 * My webserver has bare repository, `project.git`, with a post-receive hook
   that runs `minimizer project.git /var/www/docs.ruuda.nl/project`.
 * I deploy docs with `mkdocs gh-deploy --remote-name webserver:project.git`.
 * The hook minimizes any changed pages, and then checks out the result in my
   web root.

[hook]: https://git-scm.com/book/en/v2/Customizing-Git-Git-Hooks

## Building

You can do a regular build with Cargo, although it may not be very portable, as
it depends on e.g. `libbrotlienc`, `libgit2`, etc. Alternatively, you can build
a static executable with Nix (2.10 or later, for flake support):

    $ nix build
    $ ldd result/bin/minimizer
    statically linked

The static binary works on [Flatcar][flatcar] without the need to install any
dependencies, so it works well in combination with a [Miniserver][miniserver]
deployment of Nginx on Flatcar.

[flatcar]: https://www.flatcar.org/
[miniserver]: https://github.com/ruuda/miniserver

## License

Minimizer is licensed under the [Apache 2.0][apache2] license.

[apache2]: https://www.apache.org/licenses/LICENSE-2.0
