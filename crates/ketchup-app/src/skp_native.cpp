// Native SKP topology adapter. The parser is vendored, MIT-licensed OpenSKP.
// This C boundary owns its returned string; every exception is caught here.
#include "internal.hpp"
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <set>
#include <stdexcept>

namespace {
using namespace openskp;
using Json = JsonValue;
using Id = std::int64_t;
struct Import {
  const SkpModel& source;
  Json vertices=Json::make_object(), edges=Json::make_object(), faces=Json::make_object();
  Json colors=Json::make_object(), objects=Json::make_object(), hidden=Json::make_array();
  std::map<EntityId, const Material*> materials;
  std::set<std::string> hidden_layers;
  std::set<EntityId> active;
  Id next=0;
  explicit Import(const SkpModel& model):source(model), materials(model.materials_by_id()) {
    for (const auto& layer:model.layers) if(layer.hidden) hidden_layers.insert(layer.name);
  }
  Id allocate() {
    if(next>=1000000) throw std::runtime_error("The imported model is too large (one million entities maximum)");
    return ++next;
  }
  static Json ids(const std::vector<Id>& values) {
    auto out=Json::make_array(); for(auto id:values) out.push(id); return out;
  }
  Id group(std::string name, std::vector<Id> entities) {
    auto id=allocate(); auto out=Json::make_object();
    out.set("id",id);out.set("name",std::move(name));out.set("kind","Group");out.set("entities",ids(entities));
    objects.set(std::to_string(id),std::move(out));return id;
  }
  std::vector<Id> visit(const Definition& def, const std::vector<std::vector<double>>& transforms,
      std::optional<EntityId> inherited, unsigned depth) {
    if(depth>64 || !active.insert(def.id).second) throw std::runtime_error("Recursive or excessively nested SketchUp components");
    std::vector<Id> entities;
    std::map<EntityId,Id> remap;
    std::map<std::pair<Id,Id>,Id> pairs;
    for(const auto& [old,v]:def.vertices) {
      auto p=Vec3{v.x,v.y,v.z};
      for(const auto& transform:transforms) {
        if(transform.empty()) continue;
        if(transform.size()!=12 && transform.size()!=13 && transform.size()!=16)
          throw std::runtime_error("Unsupported SketchUp instance transform");
        if(transform.size()==16) {
          const auto x=p[0], y=p[1], z=p[2];
          p={transform[0]*x+transform[4]*y+transform[8]*z+transform[12],
             transform[1]*x+transform[5]*y+transform[9]*z+transform[13],
             transform[2]*x+transform[6]*y+transform[10]*z+transform[14]};
        } else p=transform_point(transform,p);
      }
      auto position=Json::make_array();
      for(auto n:p) {
        n*=0.0254;
        if(!std::isfinite(n) || std::abs(n)>1e10) throw std::runtime_error("Invalid SketchUp coordinates");
        position.push(n);
      }
      const auto id=allocate();remap[old]=id;
      auto out=Json::make_object();out.set("id",id);out.set("position",std::move(position));
      vertices.set(std::to_string(id),std::move(out));
    }
    for(const auto& [old,e]:def.edges) {
      if(!remap.count(e.v1_id) || !remap.count(e.v2_id)) throw std::runtime_error("SketchUp edge refers to a missing vertex");
      const auto a=remap.at(e.v1_id), b=remap.at(e.v2_id);
      auto key=std::make_pair(std::min(a,b),std::max(a,b));
      auto found=pairs.find(key);
      if(found!=pairs.end()) continue;
      const auto id=allocate();pairs[key]=id;entities.push_back(id);
      auto out=Json::make_object();out.set("id",id);out.set("a",a);out.set("b",b);
      edges.set(std::to_string(id),std::move(out));if(e.hidden) hidden.push(id);
    }
    for(const auto& [old,f]:def.faces) {
      std::vector<std::vector<Id>> rings;
      for(const auto& loop:f.loops) {
        if(loop.size()<3) throw std::runtime_error("SketchUp face has an invalid boundary");
        std::vector<Id> ring;
        for(std::size_t i=0;i<loop.size();++i) {
          const auto& ce=loop[i];const auto& ne=loop[(i+1)%loop.size()];
          if(!def.edges.count(ce.edge_id) || !def.edges.count(ne.edge_id)) throw std::runtime_error("SketchUp face refers to a missing edge");
          const auto& e=def.edges.at(ce.edge_id); const auto& next=def.edges.at(ne.edge_id);
          auto start=ce.orientation>=0?e.v1_id:e.v2_id;
          auto end=ce.orientation>=0?e.v2_id:e.v1_id;
          if(end!=(ne.orientation>=0?next.v1_id:next.v2_id)) throw std::runtime_error("SketchUp face has a disconnected boundary");
          ring.push_back(remap.at(start));
        }
        rings.push_back(std::move(ring));
      }
      if(rings.empty()) throw std::runtime_error("SketchUp face has an invalid boundary");
      const auto id=allocate();entities.push_back(id);
      auto out=Json::make_object(), holes=Json::make_array();
      for(std::size_t i=1;i<rings.size();++i) holes.push(ids(rings[i]));
      out.set("id",id);out.set("vertices",ids(rings[0]));out.set("holes",std::move(holes));
      faces.set(std::to_string(id),std::move(out));
      if(f.hidden || hidden_layers.count(f.layer)) hidden.push(id);
      auto material=f.material_id?f.material_id:inherited;
      if(material) {
        auto m=materials.find(*material);
        if(m!=materials.end()) {
          auto color=Json::make_array();for(unsigned j=0;j<3;++j)color.push(int(m->second->color[j]));
          colors.set(std::to_string(id),std::move(color));
        }
      }
    }
    for(const auto& instance:def.instances) {
      if(!instance.ref_idx || !source.definitions.count(*instance.ref_idx)) throw std::runtime_error("SketchUp instance refers to a missing definition");
      const auto& child=source.definitions.at(*instance.ref_idx);
      auto chain=transforms;chain.insert(chain.begin(),instance.matrix);
      auto children=visit(child,chain,instance.material_id?instance.material_id:inherited,depth+1);
      if(children.empty())continue;
      auto name=!instance.name.empty()?instance.name:(!child.name.empty()?child.name:"SketchUp group");
      auto id=group(name,std::move(children));entities.push_back(id);
      if(instance.hidden || hidden_layers.count(instance.layer))hidden.push(id);
    }
    active.erase(def.id);return entities;
  }
  std::string run() {
    auto entities=visit(source.root(),{},std::nullopt,0);
    if(entities.empty())throw std::runtime_error("This SketchUp file contains no importable edges or faces");
    group("SketchUp import",std::move(entities));
    auto out=Json::make_object();
    out.set("vertices",std::move(vertices));out.set("edges",std::move(edges));out.set("faces",std::move(faces));
    out.set("face_colors",std::move(colors));out.set("objects",std::move(objects));out.set("hidden",std::move(hidden));out.set("next_id",next);
    return to_json_string(out,0);
  }
};
char* copy(const std::string& value) {
  auto* out=static_cast<char*>(std::malloc(value.size()+1));
  if(out)std::memcpy(out,value.c_str(),value.size()+1);
  return out;
}
}
extern "C" char* ketchup_skp_parse(const unsigned char* bytes, std::size_t size, int* success) noexcept {
  *success=0;
  try {
    openskp::ByteBuffer input(bytes,bytes+size);
    auto model=openskp::build_model(openskp::full_parse(input,{}),{});
    auto json=Import(model).run();
    auto* out=copy(json);if(out)*success=1;return out;
  } catch(const std::exception& error) {return copy(error.what());}
  catch(...) {return copy("The SketchUp reader could not complete this import");}
}
extern "C" void ketchup_skp_free(char* value) noexcept {std::free(value);}
