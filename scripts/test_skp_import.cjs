// Run with node --test scripts/test_skp_import.cjs; no bundling or app build.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const base = path.resolve(__dirname, '../crates/ketchup-app');
const rust = fs.readFileSync(path.join(base, 'src/interchange.rs'), 'utf8');
const concat = rust.split('const SKP_READER: &str = concat!(')[1].split('\n);')[0];
// Read the very same concat/include_str inputs that Rust embeds.
const source = Array.from(concat.matchAll(/include_str!\("([^"]+)"\)|"(?:\\.|[^"\\])*"/g), m =>
    m[1] ? fs.readFileSync(path.resolve(base, 'src', m[1]), 'utf8') : JSON.parse(m[0])).join('');
const read = new Function('bytes', source + '\nreturn JSON.parse(ketchupImport(bytes));');
const modules = new Function(source + '\nreturn ketchupModules;')();
const sdk = modules.openskp;
const fixture = path.join(base, 'tests/fixtures/placed-window.skp');

if (process.argv.includes('--generate-fixture')) {
    const builder = sdk.create();
    const red = builder.addMaterial('Red', [210, 45, 30]);
    const window = builder.addComponentDefinition('Window panel', def => {
        def.addFace([[0,0,0],[100,0,0],[100,100,0],[0,100,0]], {
            holes: [[[25,25,0],[75,25,0],[75,75,0],[25,75,0]]], material: red,
        });
    });
    const nested = builder.addComponentDefinition('Assembly', def => {
        def.addInstance(window, {translation: [10, 0, 20]});
    });
    builder.addInstance(nested, {name: 'Placed assembly', translation: [0, 200, 0]});
    builder.addInstance(window, {name: 'Mirrored panel', translation: [300, 0, 0], matrix3x3: [-1,0,0,0,1,0,0,0,1]});
    fs.writeFileSync(fixture, builder.toBytes());
    // A minimal ZIP/VFF file with a one-inch triangle, authored here rather
    // than relying on third-party model content.
    const tlv = (tag, ...parts) => {
        const body = Buffer.concat(parts), size=Buffer.alloc(4);
        size.writeUInt32LE(body.length);
        return Buffer.concat([Buffer.from(tag,'hex'),size,body]);
    };
    const id = n => tlv('DC05', tlv('DE05', Buffer.from([n])));
    const doubles = (...values) => {const b=Buffer.alloc(values.length*8);values.forEach((v,i)=>b.writeDoubleLE(v,i*8));return b;};
    const vertex = (n,x,y,z) => tlv('C409',id(n),tlv('C509',doubles(x,y,z)));
    const edge = (n,a,b) => tlv('B80B',id(n),tlv('B90B',Buffer.from([a])),tlv('BA0B',Buffer.from([b])));
    const coedge = n => tlv('A00F',tlv('A10F',Buffer.from([n])),tlv('A20F',Buffer.from([0])));
    const face = tlv('AC0D',id(7),tlv('AD0D',doubles(0,0,1)),tlv('AE0D',tlv('9411',coedge(4),coedge(5),coedge(6))));
    const dat=tlv('F601',vertex(1,0,0,0),vertex(2,1,0,0),vertex(3,0,1,0),edge(4,1,2),edge(5,2,3),edge(6,3,1),face);
    const header=Buffer.concat([Buffer.from([255,254,255,14,0,0,255,254,255,0]),Buffer.from('{26.0.0}','utf16le')]);
    fs.writeFileSync(path.join(base,'tests/fixtures/modern-triangle.skp'),Buffer.concat([header,modules.fflate.zipSync({'model.dat':dat})]));
}

test('actual SKP bytes preserve holes, nested placement, mirrored placement, colors, and units', () => {
    const m = read(new Uint8Array(fs.readFileSync(fixture)));
    assert.equal(Object.keys(m.faces).length, 2);
    assert.equal(Object.keys(m.objects).length, 4);
    assert.equal(Object.keys(m.vertices).length, 16);
    for (const face of Object.values(m.faces)) {
        assert.equal(face.holes.length, 1);
        assert.equal(face.holes[0].length, 4);
        assert.deepEqual(m.face_colors[face.id], [210,45,30]);
    }
    const positions = Object.values(m.vertices).map(v=>v.position);
    assert(positions.some(p => Math.abs(p[0]-.254)<1e-9 && Math.abs(p[1]-5.08)<1e-9 && Math.abs(p[2]-.508)<1e-9));
    assert(positions.some(p => Math.abs(p[0]-7.62)<1e-9 && p[1] === 0 && p[2] === 0));
    assert(positions.some(p => Math.abs(p[0]-5.08)<1e-9 && p[1] === 0 && p[2] === 0));
    const all = [...Object.keys(m.vertices), ...Object.keys(m.edges), ...Object.keys(m.faces), ...Object.keys(m.objects)];
    assert.equal(new Set(all).size, all.length);
    for (const face of Object.values(m.faces)) {
        for (const ring of [face.vertices, ...face.holes]) {
            for (let i=0;i<ring.length;i++) {
                const a=ring[i],b=ring[(i+1)%ring.length];
                assert(Object.values(m.edges).some(e=>e.a===a&&e.b===b || e.a===b&&e.b===a));
            }
        }
    }
});

test('invalid and truncated files fail', () => {
    assert.throws(()=>read(new Uint8Array([1,2,3])), /header|format|invalid/i);
    assert.throws(()=>read(new Uint8Array(fs.readFileSync(fixture).subarray(0,100))));
});

test('recursive definitions and missing references fail', () => {
    const adapter=fs.readFileSync(path.join(base,'src/skp_adapter.js'),'utf8');
    const convert=new Function('source', `const ketchupModules={openskp:{parseSkp:()=>source}};${adapter};return ketchupImport(new Uint8Array(0));`);
    const root={id:1,vertices:[],edges:[],faces:[],instances:[{refIdx:1,matrix:[]}]};
    const source={root,materials:[],layers:[],definitions:new Map([[1,root]])};
    assert.throws(()=>convert(source), /recursive/i);
    source.definitions.clear();
    assert.throws(()=>convert(source), /missing definition/i);
});


test('modern ZIP/VFF SKP imports a triangle in metre coordinates', () => {
    const m=read(new Uint8Array(fs.readFileSync(path.join(base,'tests/fixtures/modern-triangle.skp'))));
    assert.equal(Object.keys(m.vertices).length,3);
    assert.equal(Object.keys(m.edges).length,3);
    assert.equal(Object.keys(m.faces).length,1);
    assert(Object.values(m.vertices).some(v=>Math.abs(v.position[0]-.0254)<1e-12));
    assert.equal(Object.keys(m.face_colors).length,0);
});

// Exercise the actual browser-worker launch body using Node's real worker
// threads with a small Worker/Blob-URL facade. No application compilation.
function workerHost() {
    const {Worker: NodeWorker} = require('node:worker_threads');
    const urls=new Map();
    const host={created:0,terminated:0,revoked:0};
    const URL={
        createObjectURL(blob) {const id='blob:test-'+urls.size;urls.set(id,blob);return id;},
        revokeObjectURL(id) {urls.delete(id);host.revoked++;},
    };
    class Worker {
        constructor(url) {
            this.pending=[];host.created++;
            urls.get(url).text().then(code=>{
                this.worker=new NodeWorker(`const {parentPort}=require('node:worker_threads');
                    globalThis.onmessage=null;globalThis.postMessage=data=>parentPort.postMessage(data);
                    ${code}
                    parentPort.on('message',data=>onmessage({data}));`,{eval:true});
                this.worker.on('message',data=>this.onmessage({data}));
                this.worker.on('error',error=>this.onerror({message:error.message}));
                for (const [data,transfer] of this.pending)this.worker.postMessage(data,transfer);
                this.pending=[];
            });
        }
        postMessage(data,transfer) {
            if(this.worker)this.worker.postMessage(data,transfer);
            else this.pending.push([data,transfer]);
        }
        terminate() {host.terminated++;this.worker.terminate();}
    }
    const body=fs.readFileSync(path.join(base,'src/skp_worker.js'),'utf8');
    const launch=new Function('source','bytes','URL','Blob','Worker',body);
    host.run=bytes=>launch(source,bytes,URL,Blob,Worker);
    return host;
}

test('worker parses off the main thread, transfers bytes, and releases resources', async () => {
    const host=workerHost();const bytes=new Uint8Array(fs.readFileSync(fixture));
    let timerRan=false;setTimeout(()=>timerRan=true,0);
    const model=JSON.parse(await host.run(bytes));
    assert(timerRan,'main-thread timers must continue during import');
    assert.equal(bytes.byteLength,0,'input buffer should be transferred');
    assert.equal(Object.keys(model.faces).length,2);
    assert.equal(host.created,1);assert.equal(host.terminated,1);assert.equal(host.revoked,1);
});

test('worker parse errors reject and release resources', async () => {
    const host=workerHost();
    await assert.rejects(host.run(new Uint8Array([1,2,3])),/header|format|invalid/i);
    assert.equal(host.terminated,1);assert.equal(host.revoked,1);
});

for (const failure of ['constructor', 'postMessage', 'error', 'messageerror']) {
    test(`worker ${failure} failure rejects and releases resources`, async () => {
        let revoked = 0, terminated = 0;
        const URL = {createObjectURL: () => 'blob:audit', revokeObjectURL: () => revoked++};
        class Worker {
            constructor() { if (failure === 'constructor') throw new Error('worker unavailable'); }
            terminate() { terminated++; }
            postMessage() {
                if (failure === 'postMessage') throw new Error('transfer failed');
                queueMicrotask(() => failure === 'error'
                    ? this.onerror({message: 'worker crashed'}) : this.onmessageerror());
            }
        }
        const launch = new Function('source','bytes','URL','Blob','Worker',
            fs.readFileSync(path.join(base,'src/skp_worker.js'),'utf8'));
        await assert.rejects(launch('', new Uint8Array([1]), URL, Blob, Worker));
        assert.equal(revoked, 1);
        assert.equal(terminated, failure === 'constructor' ? 0 : 1);
    });
}
