// Convert OpenSKP's raw, inch/Z-up topology into an isolated KetchUp model.
// Every placed definition gets its own vertices and editable group.
function ketchupImport(bytes) {
    const source = ketchupModules.openskp.parseSkp(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
    const model = { vertices: {}, edges: {}, faces: {}, face_colors: {}, objects: {}, hidden: [], next_id: 0 };
    const materials = new Map(source.materials.filter(m => m.id != null).map(m => [m.id, m.color]));
    const hiddenLayers = new Set(source.layers.filter(l => l.hidden).map(l => l.name));
    const active = new Set();
    const allocate = () => {
        if (model.next_id >= 1000000) throw new Error('The imported model is too large (one million entities maximum)');
        return ++model.next_id;
    };
    const point = (v, transforms) => {
        let p = [v.x, v.y, v.z];
        for (const m of transforms) {
            if (!m || m.length === 0) continue;
            const [x, y, z] = p;
            if (m.length === 16) p = [m[0]*x+m[4]*y+m[8]*z+m[12], m[1]*x+m[5]*y+m[9]*z+m[13], m[2]*x+m[6]*y+m[10]*z+m[14]];
            else if (m.length === 12 || m.length === 13) p = [m[0]*x+m[1]*y+m[2]*z+m[9], m[3]*x+m[4]*y+m[5]*z+m[10], m[6]*x+m[7]*y+m[8]*z+m[11]];
            else throw new Error('Unsupported SketchUp instance transform');
        }
        p = p.map(n => n * 0.0254);
        if (p.some(n => !Number.isFinite(n) || Math.abs(n) > 1e10)) throw new Error('Invalid SketchUp coordinates');
        return p;
    };
    function visit(def, transforms, inheritedMaterial, depth) {
        if (depth > 64 || active.has(def.id)) throw new Error('Recursive or excessively nested SketchUp components');
        active.add(def.id);
        const entities = [], vertices = new Map(), edges = new Map(), pairs = new Map();
        for (const v of def.vertices) {
            const id = allocate();
            vertices.set(v.id, id);
            model.vertices[id] = {id, position: point(v, transforms)};
        }
        for (const e of def.edges) {
            const a = vertices.get(e.v1Id), b = vertices.get(e.v2Id);
            if (a === undefined || b === undefined) throw new Error('SketchUp edge refers to a missing vertex');
            const key = a < b ? a + ':' + b : b + ':' + a;
            let id = pairs.get(key);
            if (id === undefined) {
                id = allocate();
                pairs.set(key, id);
                model.edges[id] = {id, a, b};
                entities.push(id);
                // Soft/smooth edges remain topology; only explicit hiding is preserved.
                if (e.hidden) model.hidden.push(id);
            }
            edges.set(e.id, e);
        }
        for (const f of def.faces) {
            const rings = f.loops.map(loop => loop.map((ce, i) => {
                const e = edges.get(ce.edgeId);
                if (!e) throw new Error('SketchUp face refers to a missing edge');
                const next = edges.get(loop[(i + 1) % loop.length].edgeId);
                const end = ce.orientation >= 0 ? e.v2Id : e.v1Id;
                const start = ce.orientation >= 0 ? e.v1Id : e.v2Id;
                const nextStart = next && (loop[(i + 1) % loop.length].orientation >= 0 ? next.v1Id : next.v2Id);
                if (end !== nextStart) throw new Error('SketchUp face has a disconnected boundary');
                return vertices.get(start);
            }));
            if (!rings.length || rings.some(r => r.length < 3)) throw new Error('SketchUp face has an invalid boundary');
            const id = allocate();
            model.faces[id] = {id, vertices: rings[0], holes: rings.slice(1)};
            entities.push(id);
            if (f.hidden || hiddenLayers.has(f.layer)) model.hidden.push(id);
            const c = materials.get(f.materialId ?? inheritedMaterial);
            if (c) model.face_colors[id] = [c.r, c.g, c.b];
        }
        for (const instance of def.instances) {
            const child = source.definitions.get(instance.refIdx);
            if (!child) throw new Error('SketchUp instance refers to a missing definition');
            const children = visit(child, [instance.matrix, ...transforms], instance.materialId ?? inheritedMaterial, depth + 1);
            if (children.length) {
                const id = allocate();
                model.objects[id] = {id, name: instance.name || child.name || 'SketchUp group', kind: 'Group', entities: children};
                entities.push(id);
                if (instance.hidden || hiddenLayers.has(instance.layer)) model.hidden.push(id);
            }
        }
        active.delete(def.id);
        return entities;
    }
    const entities = visit(source.root, [], null, 0);
    if (!entities.length) throw new Error('This SketchUp file contains no importable edges or faces');
    const id = allocate();
    model.objects[id] = {id, name: 'SketchUp import', kind: 'Group', entities};
    return JSON.stringify(model);
}
