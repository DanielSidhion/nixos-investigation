This is a repository to accompany [this blog post](https://sidhion.com/blog/posts/nixos_server_issues) that I made.

It contains a tool I built to give me a CSV to investigate the disk size used by packages and their dependencies with Nix-built software.
The tool also generates a graphviz file, but for large builds it gets quite difficult to navigate, even with some visualiser that adds interactivity to the graph.

This repository also contains the config I reached at the end of the blog post.
You can build it with `nix build .#systems.bare.toplevel`.

Note: I haven't really tested if the config still generates a working system because I gave up on that.
