{
  description = "Scatterer Herdr plugin";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    lefthook-nix = {
      url = "github:sudosubin/lefthook.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      treefmt-nix,
      lefthook-nix,
      ...
    }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      forAllSystems =
        function:
        nixpkgs.lib.genAttrs systems (
          system:
          let
            pkgs = nixpkgs.legacyPackages.${system};

            # One formatter config shared by `nix fmt`, the pre-commit hook,
            # and `nix flake check`.
            treefmtEval = treefmt-nix.lib.evalModule pkgs {
              projectRootFile = "flake.nix";
              programs.rustfmt.enable = true;
              programs.nixfmt.enable = true;
              programs.taplo.enable = true;
              programs.shfmt.enable = true;
            };

            treefmt = treefmtEval.config.build.wrapper;

            # Git hooks are generated from this config and installed by the
            # devshell's shellHook (via direnv, entering the repo is enough).
            lefthook = lefthook-nix.lib.${system}.run {
              src = self;
              config = {
                pre-commit.commands.treefmt = {
                  # --fail-on-change blocks the commit; fixed files are left
                  # in the working tree to stage and retry.
                  run = "${pkgs.lib.getExe treefmt} --fail-on-change --no-cache {staged_files}";
                };
              };
            };
          in
          function {
            inherit
              pkgs
              system
              treefmtEval
              treefmt
              lefthook
              ;
          }
        );
    in
    {
      packages = forAllSystems (
        { pkgs, ... }:
        let
          inherit (pkgs) lib;

          scatterer = pkgs.rustPlatform.buildRustPackage {
            pname = "scatterer";
            version = (lib.importTOML ./Cargo.toml).package.version;
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            buildInputs = lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
            # The git module's tests exercise real `git init/commit` in temp dirs.
            nativeCheckInputs = [ pkgs.git ];
            meta = {
              description = "Daniel's Herdr workflow/layout plugin";
              homepage = "https://github.com/RestartDK/scatterer";
              mainProgram = "scatterer";
            };
          };

          # The checked-in manifest targets a development checkout: actions run
          # `bash scripts/scatterer.sh …`, which rebuilds stale binaries via
          # cargo. None of that applies to an immutable store path, so the
          # plugin package rewrites the manifest as data: every command invokes
          # the built binary directly and the cargo build hook is dropped.
          manifest = lib.importTOML ./herdr-plugin.toml;
          storeCommand = command: [ (lib.getExe scatterer) ] ++ lib.drop 2 command;
          storeManifest = (pkgs.formats.toml { }).generate "herdr-plugin.toml" (
            builtins.removeAttrs manifest [ "build" ]
            // {
              actions = map (action: action // { command = storeCommand action.command; }) manifest.actions;
              panes = map (pane: pane // { command = storeCommand pane.command; }) manifest.panes;
            }
          );

          plugin = pkgs.runCommand "scatterer-herdr-plugin-${manifest.version}" { } ''
            mkdir -p $out/bin $out/share/herdr/plugins/scatterer
            ln -s ${lib.getExe scatterer} $out/bin/scatterer
            ln -s ${storeManifest} $out/share/herdr/plugins/scatterer/herdr-plugin.toml
          '';
        in
        {
          inherit scatterer plugin;
          default = scatterer;
        }
      );

      formatter = forAllSystems ({ treefmt, ... }: treefmt);

      checks = forAllSystems (
        { system, treefmtEval, ... }:
        {
          build = self.packages.${system}.default;
          plugin = self.packages.${system}.plugin;
          # Same treefmt config as `nix fmt` and the pre-commit hook, run
          # against a writable copy of the tree.
          formatting = treefmtEval.config.build.check self;
        }
      );

      devShells = forAllSystems (
        {
          pkgs,
          treefmt,
          lefthook,
          ...
        }:
        {
          default = pkgs.mkShell {
            # Installs the generated git hooks on shell entry.
            inherit (lefthook) shellHook;

            packages =
              with pkgs;
              [
                cargo
                clippy
                pkg-config
                rustc
                rustfmt
                treefmt
              ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                libiconv
              ];

            # Run hooks with TERM=dumb: lefthook's TUI library probes the
            # terminal with OSC escape sequences, which garbles modern
            # terminal emulators when git fires the hook. Scoped to the hook
            # invocation only; interactive `lefthook` keeps normal output.
            LEFTHOOK_BIN = toString (
              pkgs.writeShellScript "lefthook-dumb-term" ''
                exec env TERM=dumb ${pkgs.lib.getExe pkgs.lefthook} "$@"
              ''
            );
          };
        }
      );
    };
}
