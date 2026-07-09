import { readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const defaultRoot = join(scriptDir, "..");

const defaultFiles = {
  packageJson: "package.json",
  packageLock: "package-lock.json",
  tauriConfig: "src-tauri/tauri.conf.json",
  cargoToml: "src-tauri/Cargo.toml",
  cargoLock: "src-tauri/Cargo.lock",
};

const semverPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

function parseVersion(version) {
  const match = semverPattern.exec(version);
  if (!match) {
    throw new Error(`Invalid version "${version}". Expected MAJOR.MINOR.PATCH.`);
  }

  return match.slice(1).map(Number);
}

export function bumpVersion(currentVersion, bump) {
  if (semverPattern.test(bump)) {
    return bump;
  }

  const [major, minor, patch] = parseVersion(currentVersion);
  switch (bump) {
    case "patch":
      return `${major}.${minor}.${patch + 1}`;
    case "minor":
      return `${major}.${minor + 1}.0`;
    case "major":
      return `${major + 1}.0.0`;
    default:
      throw new Error(`Unknown bump "${bump}". Use patch, minor, major, or an explicit MAJOR.MINOR.PATCH version.`);
  }
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function replaceJsonVersionOccurrences(contents, from, to, count, label) {
  const pattern = new RegExp(`("version"\\s*:\\s*)"${escapeRegExp(from)}"`, "g");
  let replacements = 0;
  const nextContents = contents.replace(pattern, (match, prefix) => {
    if (replacements >= count) {
      return match;
    }

    replacements += 1;
    return `${prefix}"${to}"`;
  });

  if (replacements !== count) {
    throw new Error(`Expected to update ${count} version field(s) in ${label}, updated ${replacements}.`);
  }

  return nextContents;
}

function packageVersionFromCargoToml(contents) {
  const packageSection = contents.match(/(^\[package\][\s\S]*?)(?=\n\[|$)/);
  const match = packageSection?.[1].match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error("Could not find [package] version in Cargo.toml.");
  }
  return match[1];
}

function replacePackageVersionInCargoToml(contents, nextVersion) {
  return contents.replace(/(^\[package\][\s\S]*?^version\s*=\s*)"[^"]+"/m, `$1"${nextVersion}"`);
}

function packageVersionFromCargoLock(contents) {
  const match = contents.match(/\[\[package\]\]\nname = "launcher"\nversion = "([^"]+)"/);
  if (!match) {
    throw new Error('Could not find launcher package version in Cargo.lock.');
  }
  return match[1];
}

function replacePackageVersionInCargoLock(contents, nextVersion) {
  return contents.replace(
    /(\[\[package\]\]\nname = "launcher"\nversion = )"[^"]+"/,
    `$1"${nextVersion}"`,
  );
}

function assertSameVersions(entries) {
  const versions = new Set(entries.map((entry) => entry.version));
  if (versions.size === 1) {
    return;
  }

  const details = entries.map((entry) => `${entry.name}: ${entry.version}`).join("\n");
  throw new Error(`Version sources are not in sync:\n${details}`);
}

export async function run(args = process.argv.slice(2), options = {}) {
  const bump = args[0];
  if (!bump) {
    throw new Error("Usage: npm run bump-version -- <patch|minor|major|MAJOR.MINOR.PATCH>");
  }

  const root = options.root ?? defaultRoot;
  const files = { ...defaultFiles, ...options.files };

  const packageJsonPath = join(root, files.packageJson);
  const packageLockPath = join(root, files.packageLock);
  const tauriConfigPath = join(root, files.tauriConfig);
  const cargoTomlPath = join(root, files.cargoToml);
  const cargoLockPath = join(root, files.cargoLock);

  const packageJsonText = await readFile(packageJsonPath, "utf8");
  const packageLockText = await readFile(packageLockPath, "utf8");
  const tauriConfigText = await readFile(tauriConfigPath, "utf8");
  const packageJson = JSON.parse(packageJsonText);
  const packageLock = JSON.parse(packageLockText);
  const tauriConfig = JSON.parse(tauriConfigText);
  const cargoToml = await readFile(cargoTomlPath, "utf8");
  const cargoLock = await readFile(cargoLockPath, "utf8");

  const currentVersions = [
    { name: files.packageJson, version: packageJson.version },
    { name: `${files.packageLock} root`, version: packageLock.version },
    { name: `${files.packageLock} package`, version: packageLock.packages?.[""]?.version },
    { name: files.tauriConfig, version: tauriConfig.version },
    { name: files.cargoToml, version: packageVersionFromCargoToml(cargoToml) },
    { name: files.cargoLock, version: packageVersionFromCargoLock(cargoLock) },
  ];

  assertSameVersions(currentVersions);

  const from = packageJson.version;
  const to = bumpVersion(from, bump);

  if (from === to) {
    return { from, to };
  }

  await writeFile(packageJsonPath, replaceJsonVersionOccurrences(packageJsonText, from, to, 1, files.packageJson));
  await writeFile(packageLockPath, replaceJsonVersionOccurrences(packageLockText, from, to, 2, files.packageLock));
  await writeFile(tauriConfigPath, replaceJsonVersionOccurrences(tauriConfigText, from, to, 1, files.tauriConfig));
  await writeFile(cargoTomlPath, replacePackageVersionInCargoToml(cargoToml, to));
  await writeFile(cargoLockPath, replacePackageVersionInCargoLock(cargoLock, to));

  return { from, to };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  run()
    .then(({ from, to }) => {
      console.log(`Bumped version from ${from} to ${to}.`);
    })
    .catch((error) => {
      console.error(error.message);
      process.exitCode = 1;
    });
}
