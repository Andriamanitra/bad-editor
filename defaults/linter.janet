(defn- severity-from [str]
  (let [s (-> str (string/ascii-lower) (string/triml))]
    (cond (string/has-prefix? "info" s) :info
          (string/has-prefix? "note" s) :info
          (string/has-prefix? "warning" s) :warning
          (string/has-prefix? "error" s) :error
          :warning)))

(defn- peg/from-lint-format [lint-format]
  (peg/compile
   ~(* ,;(seq [x :in lint-format]
           (match x
             :filename ~(* (constant :filename) (<- (some (if-not (set ": \t\r\n\0\f\v") 1))))
             :line ~(* (constant :line) (number :d+))
             :column ~(* (constant :column) (number :d+))
             :message ~(* (constant :message) (<- (some (if-not (set "\r\n\0") 1)) :message))
             [:severity :from-message] ~(* (constant :severity) (cmt (backref :message) ,severity-from))
             [:severity lvl] ~(* (constant :severity) (constant ,lvl))
             _ x)))))

(defn- lint-with [& linters]
  (catseq [linter :in linters]
    (with [tempf (file/temp)]
          (os/execute (linter :command) :p {:out tempf :err tempf})
          (file/seek tempf :set 0)
          (let [lint-peg (peg/from-lint-format (linter :lint-format))]
            (seq [line :in (file/lines tempf) :let [lint (peg/match lint-peg line)] :when lint]
              (let [L (struct ;lint)]
                (print (L :filename) ":" (L :line) ":" (L :column) ":" (L :severity) ":" (L :message))))))))

(defn lint [LANGUAGE FILENAME]
  (let [
    cargo-clippy {
      :command ["cargo" "clippy" "--message-format=short"]
      :lint-format [:filename ":" :line ":" :column ": " :message [:severity :from-message]]
    }
    ruff {
      :command ["uvx" "ruff" "check" "--output-format=concise" FILENAME]
      :lint-format [:filename ":" :line ":" :column ": " :message [:severity :warning]]
    }
    mypy {
      :command ["uvx" "mypy" "--strict" FILENAME]
      :lint-format [:filename ":" :line ": " :message [:severity :from-message]]
    }
    gcc {
      :command ["gcc" "-fsyntax-only" "-fdiagnostics-plain-output" "-Wall" "-Wextra" FILENAME]
      :lint-format [:filename ":" :line ":" :column ": " :message [:severity :from-message]]
    }
   ]
    (case LANGUAGE
     :rust (lint-with cargo-clippy)
     :python (lint-with ruff mypy)
     :c (lint-with gcc)
     (eprint "no linter for " LANGUAGE)
    )))
