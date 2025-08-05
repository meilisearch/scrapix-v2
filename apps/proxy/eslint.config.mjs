import eslint from '@eslint/js'
import tseslint from '@typescript-eslint/eslint-plugin'
import tseslintParser from '@typescript-eslint/parser'
import prettier from 'eslint-plugin-prettier'

const config = [
  {
    ignores: [
      'node_modules/**',
      'dist/**',
      'coverage/**',
      '*.js',
      '*.cjs',
      '*.mjs'
    ]
  },
  eslint.configs.recommended,
  {
    files: ['**/*.ts'],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
      parser: tseslintParser,
      parserOptions: {
        project: ['./tsconfig.json'],
        tsconfigRootDir: import.meta.dirname
      },
      globals: {
        process: 'readonly',
        console: 'readonly',
        __dirname: 'readonly',
        __filename: 'readonly',
        Buffer: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
        clearInterval: 'readonly',
        NodeJS: 'readonly',
        require: 'readonly',
        module: 'readonly',
        exports: 'readonly'
      }
    },
    plugins: {
      '@typescript-eslint': tseslint,
      'prettier': prettier
    },
    rules: {
      // Prettier configuration - enforce single quotes and no semicolons
      'prettier/prettier': [
        'error',
        {
          singleQuote: true,
          semi: false,
          arrowParens: 'always',
          bracketSpacing: true,
          trailingComma: 'es5',
          printWidth: 100,
          tabWidth: 2,
          useTabs: false
        }
      ],
      
      // TypeScript specific rules
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/explicit-function-return-type': 'off',
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-unsafe-argument': 'off',
      '@typescript-eslint/no-misused-promises': 'off',
      '@typescript-eslint/ban-ts-comment': 'off',
      
      // General rules
      'no-unused-vars': 'off',
      '@typescript-eslint/no-unused-vars': ['warn', { 
        'argsIgnorePattern': '^_',
        'varsIgnorePattern': '^_',
        'caughtErrorsIgnorePattern': '^_'
      }],
      
      // Enforce consistent coding style
      'quotes': 'off',
      '@typescript-eslint/quotes': ['error', 'single', { 'avoidEscape': true }],
      'semi': 'off',
      '@typescript-eslint/semi': ['error', 'never'],
      
      // Proxy server specific rules
      'no-console': 'off', // Allow console for proxy logging
      'prefer-const': 'error',
      'no-var': 'error',
      'object-shorthand': 'error',
      'prefer-arrow-callback': 'error'
    }
  }
]

export default config