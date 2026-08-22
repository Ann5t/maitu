import packageInfo from "/opt/fudian-playwright/node_modules/playwright/package.json" with { type: "json" };
process.stdout.write(`node ${process.version}; playwright ${packageInfo.version}`);
