#!/usr/bin/env python3

import argparse
import json
from pathlib import Path


def normalize_version(raw: str) -> str:
    value = raw.strip()
    if not value:
        raise ValueError("version is required")
    return value[1:] if value.startswith("v") else value


def build_urls(public_base_url: str, asset_prefix: str, version: str) -> dict:
    base = public_base_url.rstrip("/")
    download_root = f"{base}/{asset_prefix}/releases/download/{version}"
    release_page = f"{base}/{asset_prefix}/releases/tag/{version}"
    return {
        "download_root": download_root,
        "release_page": release_page,
        "portable_exe": f"{download_root}/{asset_prefix}-{version}-x86_64.exe",
        "legacy_install_exe": f"{download_root}/{asset_prefix}-{version}-install.exe",
        "msi": f"{download_root}/{asset_prefix}-{version}-x86_64.msi",
    }


def build_manifest(
    version: str,
    public_base_url: str,
    asset_prefix: str,
    use_release_page: bool,
) -> dict:
    urls = build_urls(public_base_url, asset_prefix, version)
    update_entry = urls["release_page"] if use_release_page else urls["portable_exe"]
    return {
        "version": version,
        "downloads": {
            "windows": {
                "x86_64": {
                    "exe": update_entry,
                    "directExe": urls["portable_exe"],
                    "msi": urls["msi"],
                    "releasePage": urls["release_page"],
                },
                "install": {
                    "exe": urls["legacy_install_exe"],
                    "msi": urls["msi"],
                },
            }
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate HDesk Windows release latest.json metadata."
    )
    parser.add_argument("--version", required=True, help="Release version or tag")
    parser.add_argument(
        "--public-base-url",
        required=True,
        help="Public releases base URL, for example https://releases.hdesk.yunjichuangzhi.cn",
    )
    parser.add_argument(
        "--asset-prefix",
        default="hdesk",
        help="Asset prefix, defaults to hdesk",
    )
    parser.add_argument(
        "--direct-exe-update",
        action="store_true",
        help="Write downloads.windows.x86_64.exe as a direct EXE URL instead of a release page URL",
    )
    parser.add_argument("--output", required=True, help="Output latest.json path")
    args = parser.parse_args()

    version = normalize_version(args.version)
    manifest = build_manifest(
        version=version,
        public_base_url=args.public_base_url,
        asset_prefix=args.asset_prefix,
        use_release_page=not args.direct_exe_update,
    )

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()