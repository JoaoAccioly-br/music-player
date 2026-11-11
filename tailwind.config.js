/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{vue,js,ts,jsx,tsx}',
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        ytm: {
          bg: '#030303',
          surface: '#121212',
          elevated: '#1d1d1d',
          border: '#272727',
          red: '#ff0000',
          accent: '#ff4e45',
        },
      },
    },
  },
  plugins: [],
}
