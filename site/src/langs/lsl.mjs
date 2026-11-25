const comments = {
  patterns: [
    { begin: '//', end: '$', name: 'comment.line.double-slash.lsl' },
    { begin: '/\\*', end: '\\*/', name: 'comment.block.lsl' },
  ],
};

const strings = {
  patterns: [
    {
      begin: '"',
      end: '"',
      name: 'string.quoted.double.lsl',
      patterns: [{ match: '\\\\.', name: 'constant.character.escape.lsl' }],
    },
  ],
};

const lsl = {
  name: 'lsl',
  scopeName: 'source.lsl',
  patterns: [
    { include: '#comments' },
    { include: '#strings' },
    { include: '#constants' },
    { include: '#types' },
    { include: '#keywords' },
    { include: '#numbers' },
    { match: '\\b(ll[A-Z][A-Za-z0-9_]*)\\b', name: 'support.function.lsl' },
    { match: '\\b[a-zA-Z_][A-Za-z0-9_]*(?=\\s*\\()', name: 'entity.name.function.lsl' },
    { match: '@[A-Za-z_][A-Za-z0-9_]*', name: 'entity.name.label.lsl' },
    { match: '\\b(TRUE|FALSE)\\b', name: 'constant.language.boolean.lsl' },
    { match: '==|!=|<=|>=|&&|\\|\\||\\+\\+|--|\\+=|-=|\\*=|/=|[=+*/%<>!-]', name: 'keyword.operator.lsl' },
    { match: '[{}()\\[\\],.;:]', name: 'punctuation.separator.lsl' },
  ],
  repository: {
    comments,
    strings,
    constants: {
      patterns: [
        { match: '\\b(NULL_KEY|ZERO_VECTOR|ZERO_ROTATION|PI|TWO_PI|DEG_TO_RAD|RAD_TO_DEG)\\b', name: 'constant.language.lsl' },
      ],
    },
    types: {
      patterns: [
        { match: '\\b(integer|float|string|key|vector|rotation|list)\\b', name: 'storage.type.lsl' },
      ],
    },
    keywords: {
      patterns: [
        { match: '\\b(default|state|if|else|for|while|do|return|jump|break|continue)\\b', name: 'keyword.control.lsl' },
      ],
    },
    numbers: {
      patterns: [
        { match: '\\b(?:0[xX][0-9A-Fa-f]+|\\d+(?:\\.\\d*)?(?:[eE][+-]?\\d+)?|\\.\\d+(?:[eE][+-]?\\d+)?)\\b', name: 'constant.numeric.lsl' },
      ],
    },
  },
};

export default lsl;
