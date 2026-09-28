import { execSync } from 'child_process';
import { createHash } from 'crypto';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const WORKSPACE_ROOT = process.env.WORKSPACE_ROOT ?? resolve(dirname(fileURLToPath(import.meta.url)), '../..');

// Build and run the Rust binary, capture JSON from stdout
console.log('Building Rust hash_vectors binary...');
execSync('cargo build --release --bin hash_vectors', { stdio: 'inherit', cwd: WORKSPACE_ROOT });

console.log('Running hash_vectors...');
const output = execSync('./target/release/hash_vectors', {
    cwd: WORKSPACE_ROOT,
    maxBuffer: 256 * 1024 * 1024,
});

const vectors = JSON.parse(output.toString());
console.log(`Loaded ${vectors.length} test vectors from Rust\n`);

let passed = 0;
let failed = 0;
const failures = [];

for (const { inputs, output: expected } of vectors) {
    const fields = inputs.map((n) => {
        const field = Buffer.alloc(32);
        field.writeBigUInt64BE(BigInt(n), 24);
        return field;
    });
    const result = `0x${createHash('sha256').update(Buffer.concat(fields)).digest('hex')}`;

    if (result === expected) {
        passed++;
    } else {
        failed++;
        failures.push({ inputs, expected, got: result });
        if (failures.length <= 10) {
            console.error(`FAIL inputs=${JSON.stringify(inputs)}`);
            console.error(`     expected=${expected}`);
            console.error(`     got     =${result}`);
        }
    }
}

console.log(`\nResults: ${passed} passed, ${failed} failed out of ${vectors.length} vectors`);

if (failed > 0) {
    console.error(`\nFirst failures:`);
    failures.slice(0, 10).forEach(({ inputs, expected, got }) => {
        console.error(`  inputs=${JSON.stringify(inputs)} expected=${expected} got=${got}`);
    });
    process.exit(1);
} else {
    console.log('All vectors match. Rust and Node SHA-256 are compatible. This is a desperately inadequate evalutation of the space but serves as a sanity check.');
}
