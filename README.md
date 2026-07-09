# mcmod

Declarative Minecraft mod manager.

I love [Nix](https://nixos.org/) and the whole declarative way of writing infrastructure. I wanted that for my Minecraft mods. So I made this.

Declare your mods in a YAML config, run `mcmod apply`, and it figures out the rest and resolves versions from Modrinth and CurseForge, downloads, updates, removes. No more dragging `.jar` files around.


# install

```sh
cargo install mcmod
```

# docs

```
Usage: mcmod <COMMAND>

Commands:
  init    Scan existing installation and generate config
  add     Add a mod to an instance
  remove  Remove a mod from an instance
  rename  Rename a mod slug
  list    List configured mods
  update  Re-resolve newer compatible versions
  plan    Show what would change
  apply   Reconcile all instances with the declared config
  status  Show drift between declared and actual state
  search  Search Modrinth for mods
  info    Show Modrinth project info for a mod
```
