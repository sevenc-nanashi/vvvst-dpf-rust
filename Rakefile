task "build:debug" do
  sh "cargo build"
  sh "cmake -DCMAKE_BUILD_TYPE=Debug -Bout/build/x64-Debug" unless Dir.exist?("out/build/x64-Debug")
  sh "cmake --build out/build/x64-Debug"
end

task "build:release" do
  sh "cargo build --release"
  sh "cmake -DCMAKE_BUILD_TYPE=Release -Bout/build/x64-Release" unless Dir.exist?("out/build/x64-Release")
  sh "cmake --build out/build/x64-Release"
end

task "generate-header" do
  sh "cargo xtask generate-header"
end
