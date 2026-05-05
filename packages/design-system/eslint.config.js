export default [
  {
    files: ['src/**/*.{ts,tsx}'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: ['newton-ui', 'newton-ui/*', '../../newton-ui/*'],
              message:
                'design-system MUST NOT import from newton-ui (E_DS_CIRCULAR).',
            },
          ],
        },
      ],
    },
  },
];
