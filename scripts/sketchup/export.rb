# Exports the open SketchUp model for SolidSketch (crates/model/src/import.rs): visible faces
# and component instances, definition by definition, lengths in mm.
#
# Run inside SketchUp (Ruby console, or the Claude Bridge `/eval`):
#   load '/path/to/scripts/sketchup/export.rb'
#   SolidSketchExport.run('/path/to/model.json')
#
# Format (`solidsketch-sketchup-export`, version 1):
#   materials   : [{ name, color: [r, g, b] }]
#   definitions : [{ name, group, no_scale_mask, vertices, faces, instances }]
#   root        : { vertices, faces, instances }      — the model's own entities
#   vertices    : [[x, y, z]] in mm, in the definition's axes
#   faces       : [{ normal: [x, y, z], loops: [[vertex index]] }] — the outer loop first
#   instances   : [{ definition, name, transform: 16 floats column-major (translation in mm),
#                    material }]
# Hidden entities and entities on hidden tags are left out; edges alone, images, texts and
# dimensions too.

require 'json'

module SolidSketchExport

  FORMAT = 'solidsketch-sketchup-export'.freeze
  VERSION = 1
  MM = 25.4

  def self.run(path, model = Sketchup.active_model)
    t0 = Time.now
    @definitions = []
    @definition_index = {}
    @materials = []
    @material_index = {}
    @stats = { faces: 0, instances: 0 }

    root = entities_of(model.entities)
    # Definitions are added while walking: index by index until none is left.
    i = 0
    while i < @definitions.size
      d = @definitions[i]
      @definitions[i] = {
        name: d.name,
        group: d.group?,
        no_scale_mask: d.behavior.no_scale_mask?,
      }.merge(entities_of(d.entities))
      i += 1
    end

    data = {
      format: FORMAT,
      version: VERSION,
      model: model.title,
      materials: @materials,
      definitions: @definitions,
      root: root,
    }
    File.write(path, JSON.generate(data))
    {
      path: path,
      definitions: @definitions.size,
      faces: @stats[:faces],
      instances: @stats[:instances],
      materials: @materials.size,
      bytes: File.size(path),
      seconds: (Time.now - t0).round(3),
    }
  end

  def self.visible?(e)
    !e.hidden? && (e.layer.nil? || e.layer.visible?)
  end

  def self.definition_index(d)
    @definition_index[d] ||= begin
      @definitions << d
      @definitions.size - 1
    end
  end

  def self.material_index(m)
    return nil if m.nil?
    @material_index[m] ||= begin
      @materials << { name: m.display_name, color: m.color.to_a[0, 3] }
      @materials.size - 1
    end
  end

  def self.entities_of(entities)
    vertices = []
    vertex_index = {}
    index = lambda do |v|
      vertex_index[v] ||= begin
        vertices << v.position.to_a.map { |c| (c.to_f * MM).round(6) }
        vertices.size - 1
      end
    end

    faces = []
    instances = []
    entities.each do |e|
      next unless visible?(e)
      case e
      when Sketchup::Face
        loops = [e.outer_loop] + e.loops.reject(&:outer?)
        faces << {
          normal: e.normal.to_a,
          loops: loops.map { |l| l.vertices.map { |v| index.call(v) } },
        }
      when Sketchup::ComponentInstance, Sketchup::Group
        t = e.transformation.to_a
        t[12] *= MM
        t[13] *= MM
        t[14] *= MM
        instances << {
          definition: definition_index(e.definition),
          name: e.name,
          transform: t,
          material: material_index(e.material),
        }
      end
    end
    @stats[:faces] += faces.size
    @stats[:instances] += instances.size
    { vertices: vertices, faces: faces, instances: instances }
  end

end
