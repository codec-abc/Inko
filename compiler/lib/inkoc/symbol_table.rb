# frozen_string_literal: true

module Inkoc
  class SymbolTable
    include Inspect
    include Enumerable

    attr_reader :symbols, :mapping
    attr_accessor :parent

    def initialize(parent = nil)
      @symbols = []
      @mapping = {}
      @parent = parent
    end

    def add_symbol(symbol)
      @symbols << symbol
      @mapping[symbol.name] = symbol
    end

    def define(name, type, mutable = false)
      symbol = Symbol.new(name, type, @symbols.length, mutable)

      @symbols << symbol
      @mapping[name] = symbol

      symbol
    end

    def reassign(name, type)
      self[name].type = type
    end

    def names
      @mapping.keys
    end

    def each
      return to_enum(__method__) unless block_given?

      @symbols.each do |value|
        yield value
      end
    end

    def [](name_or_index)
      source = name_or_index.is_a?(Integer) ? @symbols : @mapping

      source[name_or_index] || NullSymbol.new(name_or_index)
    end

    def slice(range)
      @symbols[range] || []
    end

    def lookup_with_parent(name_or_index)
      source = self
      depth = -1

      while source
        found = source[name_or_index]

        return [depth, found] if found.any?

        depth += 1
        source = source.parent
      end

      [-1, NullSymbol.new(name_or_index)]
    end

    def lookup_in_root(name_or_index)
      source = self
      depth = -1

      while source.parent
        depth += 1
        source = source.parent
      end

      [depth, source[name_or_index]]
    end

    def defined?(name)
      lookup_with_parent(name)[1].any?
    end

    def last
      @symbols.last
    end

    def any?(&block)
      @symbols.any?(&block)
    end

    def empty?
      @symbols.empty?
    end

    def length
      @symbols.length
    end

    def ==(other)
      other.is_a?(self.class) &&
        mapping == other.mapping &&
        parent == other.parent
    end
  end
end
