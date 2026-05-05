export default [
  {
    files: ['src/**/*.{ts,tsx}'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: ['@newton/design-system/src/*'],
              message:
                'Import from the @newton/design-system package entry, not internal paths.',
            },
          ],
        },
      ],
    },
  },
];
